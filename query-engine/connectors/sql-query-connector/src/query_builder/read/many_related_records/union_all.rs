use super::*;
use crate::ordering::Ordering;
use connector_interface::SkipAndLimit;
use prisma_models::{RecordIdentifier, SelectedFields};
use quaint::ast::*;

pub struct ManyRelatedRecordsWithUnionAll;

impl ManyRelatedRecordsQueryBuilder for ManyRelatedRecordsWithUnionAll {
    fn with_pagination(base: ManyRelatedRecordsBaseQuery) -> Query {
        let distinct_ids = {
            let mut ids = base.from_record_ids.to_vec();
            ids.dedup();

            ids
        };

        let order_columns = Ordering::internal(vec![SelectedFields::RELATED_MODEL_ALIAS], base.order_directions);
        let base_condition = base.condition.and(base.cursor);

        let base_query = match base.skip_and_limit {
            SkipAndLimit {
                skip,
                limit: Some(limit),
            } => base.query.limit(limit).offset(skip),
            SkipAndLimit { skip, limit: None } => base.query.offset(skip),
        };

        let base_query = order_columns.into_iter().fold(base_query, |acc, ord| acc.order_by(ord));
        let mut distinct_ids = distinct_ids.into_iter();

        //        let build_cond = |ids: RecordIdentifier| {
        //            let id_cond = ids.into_iter().fold(ConditionTree::NoCondition, |acc, (dsf, val)| {
        //                let col = dsf.as_column();
        //                match acc {
        //                    ConditionTree::NoCondition => col.equals(val).into(),
        //                    cond => cond.and(col.equals(val)),
        //                }
        //            });
        //
        //            let conditions = base_condition.clone().and(id_cond);
        //
        //            base_query.clone().so_that(conditions)
        //        };

        let from_field = base.from_field;
        let build_cond = |id: RecordIdentifier| {
            let conditions = base_condition
                .clone()
                .and(from_field.relation_column(true).equals(id.single_value()));

            base_query.clone().so_that(conditions)
        };

        if let Some(id) = distinct_ids.nth(0) {
            let union = distinct_ids.fold(Union::new(build_cond(id)), |acc, id| acc.all(build_cond(id)));

            Query::from(union)
        } else {
            Query::from(Union::default())
        }
    }

    fn uses_row_number() -> bool {
        false
    }
}
