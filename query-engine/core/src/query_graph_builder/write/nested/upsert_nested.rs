use super::*;
use crate::query_graph_builder::write::utils::coerce_vec;
use crate::{
    query_ast::*,
    query_graph::{Flow, Node, NodeRef, QueryGraph, QueryGraphDependency},
    InputAssertions, ParsedInputMap, ParsedInputValue,
};
use connector::{Filter, ScalarCompare};
use prisma_models::RelationFieldRef;
use std::{convert::TryInto, sync::Arc};

/// Handles a nested upsert.
/// The constructed query graph can have different shapes based on the relation
/// of parent and child and where relations are inlined:
///
/// Many-to-many relation:
/// ```text
///    ┌ ─ ─ ─ ─ ─ ─ ─ ─ ┐
///          Parent       ────────────────────────┐
///    └ ─ ─ ─ ─ ─ ─ ─ ─ ┘         │              │
///             │                                 │
///             │                  │              │
///             │                                 │
///             ▼                  ▼              │
///    ┌─────────────────┐  ┌ ─ ─ ─ ─ ─ ─         │
/// ┌──│   Read Child    │      Result   │        │
/// │  └─────────────────┘  └ ─ ─ ─ ─ ─ ─         │
/// │           │                                 │
/// │           │                                 │
/// │           │                                 │
/// │           ▼                                 │
/// │  ┌─────────────────┐                        │
/// │  │   If (exists)   │────────────┐           │
/// │  └─────────────────┘            │           │
/// │           │                     │           │
/// │           │                     │           │
/// │           │                     │           │
/// │           ▼                     ▼           │
/// │  ┌─────────────────┐   ┌─────────────────┐  │
/// └─▶│  Update Child   │   │  Create Child   │  │
///    └─────────────────┘   └─────────────────┘  │
///                                   │           │
///                                   │           │
///                                   │           │
///                                   ▼           │
///                          ┌─────────────────┐  │
///                          │     Connect     │◀─┘
///                          └─────────────────┘
/// ```
///
/// One-to-x relation:
/// ```text
///    Inlined in parent:                                     Inlined in child:
///    ┌ ─ ─ ─ ─ ─ ─ ─ ─ ┐                                    ┌ ─ ─ ─ ─ ─ ─ ─ ─ ┐
///          Parent       ────────────────────────┐                 Parent       ────────────────────────┐
///    └ ─ ─ ─ ─ ─ ─ ─ ─ ┘         │              │           └ ─ ─ ─ ─ ─ ─ ─ ─ ┘         │              │
///             │                                 │                    │                                 │
///             │                  │              │                    │                  │              │
///             │                                 │                    │                                 │
///             ▼                  ▼              │                    ▼                  ▼              │
///    ┌─────────────────┐  ┌ ─ ─ ─ ─ ─ ─         │           ┌─────────────────┐  ┌ ─ ─ ─ ─ ─ ─         │
/// ┌──│   Read Child    │      Result   │        │        ┌──│   Read Child    │      Result   │        │
/// │  └─────────────────┘  └ ─ ─ ─ ─ ─ ─         │        │  └─────────────────┘  └ ─ ─ ─ ─ ─ ─         │
/// │           │                                 │        │           │                                 │
/// │           │                                 │        │           │                                 │
/// │           │                                 │        │           │                                 │
/// │           ▼                                 │        │           ▼                                 │
/// │  ┌─────────────────┐                        │        │  ┌─────────────────┐                        │
/// │  │   If (exists)   │────────────┐           │        │  │   If (exists)   │────────────┐           │
/// │  └─────────────────┘            │           │        │  └─────────────────┘            │           │
/// │           │                     │           │        │           │                     │           │
/// │           │                     │           │        │           │                     │           │
/// │           │                     │           │        │           │                     │           │
/// │           ▼                     ▼           │        │           ▼                     ▼           │
/// │  ┌─────────────────┐   ┌─────────────────┐  │        │  ┌─────────────────┐   ┌─────────────────┐  │
/// └─▶│  Update Child   │   │  Create Child   │  │        └─▶│  Update Child   │   │  Create Child   │◀─┘
///    └─────────────────┘   └─────────────────┘  │           └─────────────────┘   └─────────────────┘
///                                   │           │
///                                   │           │
///                                   │           │
///                                   ▼           │
///                          ┌─────────────────┐  │
///                          │  Update Parent  │◀─┘
///                          └─────────────────┘
/// ```
/// Todo split this mess up and clean up the code.
pub fn connect_nested_upsert(
    graph: &mut QueryGraph,
    parent_node: NodeRef,
    parent_relation_field: &RelationFieldRef,
    value: ParsedInputValue,
) -> QueryGraphBuilderResult<()> {
    let child_model_identifier = parent_relation_field.related_model().primary_identifier();
    let child_model = parent_relation_field.related_model();

    for value in coerce_vec(value) {
        let mut as_map: ParsedInputMap = value.try_into()?;
        let create_input = as_map.remove("create").expect("create argument is missing");
        let update_input = as_map.remove("update").expect("update argument is missing");

        // Read child(ren) node
        let filter: Filter = if parent_relation_field.is_list {
            let where_input: ParsedInputMap = as_map.remove("where").expect("where argument is missing").try_into()?;

            where_input.assert_size(1)?;
            where_input.assert_non_null()?;

            extract_filter(where_input, &child_model, false)?
        } else {
            Filter::empty()
        };

        let read_children_node =
            utils::insert_find_children_by_parent_node(graph, &parent_node, parent_relation_field, filter)?;

        let create_node = create::create_record_node(graph, Arc::clone(&child_model), create_input.try_into()?)?;
        let update_node = update::update_record_node(
            graph,
            Filter::empty(),
            Arc::clone(&child_model),
            update_input.try_into()?,
        )?;
        let if_node = graph.create_node(Flow::default_if());

        graph.create_edge(
            &read_children_node,
            &if_node,
            QueryGraphDependency::ParentIds(
                child_model_identifier.clone(),
                Box::new(|node, parent_ids| {
                    if let Node::Flow(Flow::If(_)) = node {
                        Ok(Node::Flow(Flow::If(Box::new(move || !parent_ids.is_empty()))))
                    } else {
                        Ok(node)
                    }
                }),
            ),
        )?;

        let id_field = child_model.fields().find_singular_id().unwrap().upgrade().unwrap();

        graph.create_edge(
             &read_children_node,
             &update_node,
             QueryGraphDependency::ParentIds(child_model_identifier.clone(), Box::new(move |mut node, mut parent_ids| {
                 if let Node::Query(Query::Write(WriteQuery::UpdateRecord(ref mut x))) = node {
                     let parent_id = match parent_ids.pop() {
                         Some(pid) => Ok(pid),
                         None => Err(QueryGraphBuilderError::AssertionError(format!(
                             "[Query Graph] Expected a valid parent ID to be present for a nested update in a nested upsert."
                         ))),
                     }?;

                     x.add_filter(id_field.data_source_field().equals(parent_id.single_value()));
                 }
                 Ok(node)
             })),
         )?;

        graph.create_edge(&if_node, &update_node, QueryGraphDependency::Then)?;
        graph.create_edge(&if_node, &create_node, QueryGraphDependency::Else)?;

        // Specific handling based on relation type and inlining side.
        if parent_relation_field.relation().is_many_to_many() {
            // Many to many only needs a connect node.
            connect::connect_records_node(graph, &parent_node, &create_node, &parent_relation_field, 1)?;
        } else {
            if parent_relation_field.relation_is_inlined_in_parent() {
                let parent_model = parent_relation_field.model();
                let related_field_name = parent_relation_field.name.clone();

                // Update parent node
                let update_node =
                    utils::update_records_node_placeholder(graph, Filter::empty(), Arc::clone(&parent_model));
                let id_field = parent_model.fields().find_singular_id().unwrap().upgrade().unwrap();

                // Edge to retrieve the finder
                graph.create_edge(
                     &parent_node,
                     &update_node,
                     QueryGraphDependency::ParentIds(child_model_identifier.clone(), Box::new(move |mut child_node, mut parent_ids| {
                         let parent_id = match parent_ids.pop() {
                             Some(pid) => Ok(pid),
                             None => Err(QueryGraphBuilderError::AssertionError(format!(
                                 "[Query Graph] Expected a valid parent ID to be present to retrieve the finder for a parent update in a nested upsert."
                             ))),
                         }?;

                         if let Node::Query(Query::Write(ref mut wq)) = child_node {
                             wq.add_filter(id_field.data_source_field().equals(parent_id.single_value()));
                         }

                         Ok(child_node)
                     })),
                 )?;

                // Edge to retrieve the child ID to inject
                graph.create_edge(
                     &create_node,
                     &update_node,
                     QueryGraphDependency::ParentIds(child_model_identifier.clone(), Box::new(|mut child_node, mut parent_ids| {
                         let parent_id = match parent_ids.pop() {
                             Some(pid) => Ok(pid),
                             None => Err(QueryGraphBuilderError::AssertionError(format!(
                                 "[Query Graph] Expected a valid parent ID to be present to retrieve the ID to inject for a parent update in a nested upsert."
                             ))),
                         }?;

                         if let Node::Query(Query::Write(ref mut wq)) = child_node {
                             wq.inject_field_arg(related_field_name, parent_id.single_value());
                         }

                         Ok(child_node)
                     })),
                 )?;
            } else {
                // Inlined on child
                let related_field_name = parent_relation_field.related_field().name.clone();

                // Edge to retrieve the child ID to inject (inject into the create)
                graph.create_edge(
                     &parent_node,
                     &create_node,
                     QueryGraphDependency::ParentIds(child_model_identifier.clone(), Box::new(|mut child_node, mut parent_ids| {
                         let parent_id = match parent_ids.pop() {
                             Some(pid) => Ok(pid),
                             None => Err(QueryGraphBuilderError::AssertionError(format!(
                                 "[Query Graph] Expected a valid parent ID to be present to retrieve the ID to inject into the child create in a nested upsert."
                             ))),
                         }?;

                         if let Node::Query(Query::Write(ref mut wq)) = child_node {
                             wq.inject_field_arg(related_field_name, parent_id.single_value());
                         }

                         Ok(child_node)
                     })),
                 )?;
            }
        }
    }

    Ok(())
}
