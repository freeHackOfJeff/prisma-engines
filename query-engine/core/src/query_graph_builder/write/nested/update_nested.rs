use super::*;
use crate::{
    query_ast::*,
    query_graph::{Node, NodeRef, QueryGraph, QueryGraphDependency},
    InputAssertions, ParsedInputValue,
};
use connector::Filter;
use prisma_models::{ModelRef, RelationFieldRef};
use std::{convert::TryInto, sync::Arc};
use utils::IdFilter;
use write_args_parser::*;

/// Handles nested update (one) cases.
///
/// We need to reload the parent node if it doesn't yield the necessary
/// fields in the result to satisfy the relation inlining.
/// ([DTODO]] Implement reloading. Always reloading right now for simplicity)
///
/// ```text
///    ┌ ─ ─ ─ ─ ─ ─                       ┌ ─ ─ ─ ─ ─ ─
/// ┌──    Parent   │─ ─ ─ ─ ─          ┌──    Parent   │─ ─ ─ ─ ─
/// │  └ ─ ─ ─ ─ ─ ─          │         │  └ ─ ─ ─ ─ ─ ─          │
/// │         │                         │         │
/// │         ▼               ▼         │         ▼               ▼
/// │  ┌────────────┐   ┌ ─ ─ ─ ─ ─     │  ┌────────────┐   ┌ ─ ─ ─ ─ ─
/// │  │   Check    │      Result  │    │  │   Reload   │      Result  │
/// │  └────────────┘   └ ─ ─ ─ ─ ─     │  └────────────┘   └ ─ ─ ─ ─ ─
/// │         │                         │         │
/// │         ▼                         │         ▼
/// │  ┌────────────┐                   │  ┌────────────┐
/// └─▶│   Update   │                   │  │   Check    │
///    └────────────┘                   │  └────────────┘
///                                     │         │
///                                     │         ▼
///                                     │  ┌────────────┐
///                                     └─▶│   Update   │
///                                        └────────────┘
/// ```
pub fn connect_nested_update(
    graph: &mut QueryGraph,
    parent: &NodeRef,
    parent_relation_field: &RelationFieldRef,
    value: ParsedInputValue,
    child_model: &ModelRef,
) -> QueryGraphBuilderResult<()> {
    for value in utils::coerce_vec(value) {
        let (data, filter) = if parent_relation_field.is_list {
            // We have to have a single record filter in "where".
            // This is used to read the children first, to make sure they're actually connected.
            // The update itself operates on the record ID found by the read check.
            let mut map: ParsedInputMap = value.try_into()?;
            let where_arg: ParsedInputMap = map.remove("where").unwrap().try_into()?;

            where_arg.assert_size(1)?;
            where_arg.assert_non_null()?;

            let filter = extract_filter(where_arg, &child_model, false)?;
            let data_value = map.remove("data").unwrap();

            (data_value, filter)
        } else {
            (value, Filter::empty())
        };

        let find_child_records_node =
            utils::insert_find_children_by_parent_node(graph, parent, parent_relation_field, filter)?;

        let update_node =
            update::update_record_node(graph, Filter::empty(), Arc::clone(child_model), data.try_into()?)?;

        let child_model_identifier = parent_relation_field.related_model().primary_identifier();

        graph.create_edge(
            &find_child_records_node,
            &update_node,
            QueryGraphDependency::ParentIds(
                child_model_identifier.clone(),
                Box::new(move |mut node, mut parent_ids| {
                    let parent_id = match parent_ids.pop() {
                        Some(pid) => Ok(pid),
                        None => Err(QueryGraphBuilderError::AssertionError(format!(
                            "Expected a valid parent ID to be present for nested update to-one case."
                        ))),
                    }?;

                    if let Node::Query(Query::Write(WriteQuery::UpdateRecord(ref mut ur))) = node {
                        ur.add_filter(parent_id.filter());
                    }

                    Ok(node)
                }),
            ),
        )?;
    }

    Ok(())
}

pub fn connect_nested_update_many(
    graph: &mut QueryGraph,
    parent: &NodeRef,
    parent_relation_field: &RelationFieldRef,
    value: ParsedInputValue,
    child_model: &ModelRef,
) -> QueryGraphBuilderResult<()> {
    for value in utils::coerce_vec(value) {
        let mut map: ParsedInputMap = value.try_into()?;
        let where_arg = map.remove("where").unwrap();
        let data_value = map.remove("data").unwrap();
        let data_map: ParsedInputMap = data_value.try_into()?;
        let where_map: ParsedInputMap = where_arg.try_into()?;
        let child_model_identifier = parent_relation_field.related_model().primary_identifier();

        let filter = extract_filter(where_map, child_model, true)?;
        let update_args = WriteArgsParser::from(&child_model, data_map)?;

        let find_child_records_node =
            utils::insert_find_children_by_parent_node(graph, parent, parent_relation_field, filter.clone())?;

        // TODO: this looks like some duplication from write/update.rs
        let update_many = WriteQuery::UpdateManyRecords(UpdateManyRecords {
            model: Arc::clone(&child_model),
            filter,
            args: update_args.args,
        });

        let update_many_node = graph.create_node(Query::Write(update_many));

        graph.create_edge(
            &find_child_records_node,
            &update_many_node,
            QueryGraphDependency::ParentIds(
                child_model_identifier.clone(),
                Box::new(move |mut node, parent_ids| {
                    if let Node::Query(Query::Write(WriteQuery::UpdateManyRecords(ref mut ur))) = node {
                        // let conditions: QueryGraphBuilderResult<Vec<_>> =
                        // parent_ids.into_iter().try_fold(vec![], |mut acc, next| {
                        //     let assimilated = child_model_identifier.assimilate(next)?;

                        //     acc.push(assimilated.filter());
                        //     Ok(acc)
                        // });

                        // let filter = Filter::or(conditions?);
                        let filter = Filter::or(parent_ids.into_iter().map(|id| id.filter()).collect());
                        ur.set_filter(Filter::and(vec![ur.filter.clone(), filter]));
                    }

                    Ok(node)
                }),
            ),
        )?;
    }

    Ok(())
}
