use crate::{
    query_builder::{
        self,
        read::{self, ManyRelatedRecordsBaseQuery, ManyRelatedRecordsQueryBuilder},
    },
    QueryExt, SqlError,
};
use connector_interface::*;
use prisma_models::*;
use quaint::ast::*;

pub async fn get_single_record(
    conn: &dyn QueryExt,
    model: &ModelRef,
    filter: &Filter,
    selected_fields: &SelectedFields,
) -> crate::Result<Option<SingleRecord>> {
    let query = read::get_records(&model, selected_fields.columns(), filter);
    let field_names = selected_fields.db_names().map(String::from).collect();
    let idents: Vec<_> = selected_fields.types().collect();

    let record = (match conn.find(query, idents.as_slice()).await {
        Ok(result) => Ok(Some(result)),
        Err(_e @ SqlError::RecordNotFoundForWhere(_)) => Ok(None),
        Err(_e @ SqlError::RecordDoesNotExist) => Ok(None),
        Err(e) => Err(e),
    })?
    .map(Record::from)
    .map(|record| SingleRecord { record, field_names });

    Ok(record)
}

pub async fn get_many_records(
    conn: &dyn QueryExt,
    model: &ModelRef,
    query_arguments: QueryArguments,
    selected_fields: &SelectedFields,
) -> crate::Result<ManyRecords> {
    let field_names = selected_fields.db_names().map(String::from).collect();
    let idents: Vec<_> = selected_fields.types().collect();
    let query = read::get_records(model, selected_fields.columns(), query_arguments);

    let records = conn
        .filter(query.into(), idents.as_slice())
        .await?
        .into_iter()
        .map(Record::from)
        .collect();

    Ok(ManyRecords { records, field_names })
}

pub async fn get_related_records<T>(
    conn: &dyn QueryExt,
    from_field: &RelationFieldRef,
    from_record_ids: &[RecordIdentifier],
    query_arguments: QueryArguments,
    selected_fields: &SelectedFields,
) -> crate::Result<ManyRecords>
where
    T: ManyRelatedRecordsQueryBuilder,
{
    // Todo: Does this work with relation fields that are backed by multiple fields?
    // Q: What is the TypeIdentifier::Relation actually doing?
    // Q: The following code simply queries both field sides, is this correct?
    //    Columns that don't exist are ignored? Iterator is empty?
    // Q: Additionally: field names contains always both, isn't that breaking the above assumption?
    let mut idents: Vec<_> = selected_fields.types().collect();
    idents.extend(from_field.related_field().type_identifiers_with_arities());
    idents.extend(from_field.linking_fields().type_identifiers_with_arities());
    idents.extend(from_field.linking_fields().type_identifiers_with_arities());

    let field_names: Vec<String> = selected_fields
        .db_names()
        //        .chain(from_field.related_field().db_names())
        .map(String::from)
        .collect();

    //    field_names.push(from_field.name.clone());

    let can_skip_joins = from_field.relation_is_inlined_in_child() && !query_arguments.is_with_pagination();
    let mut columns: Vec<_> = selected_fields.columns().collect();
    let is_with_pagination = query_arguments.is_with_pagination();

    columns.extend(
        from_field
            .opposite_columns(true)
            .into_iter()
            .map(|col| col.alias(SelectedFields::RELATED_MODEL_ALIAS))
            .collect::<Vec<_>>(),
    );

    columns.extend(
        from_field
            .relation_columns(true)
            .into_iter()
            .map(|col| col.alias(SelectedFields::PARENT_MODEL_ALIAS))
            .collect::<Vec<_>>(),
    );

    let query = if can_skip_joins {
        let model = from_field.related_model();
        let relation_columns: Vec<_> = from_field.relation_columns(true).collect();
        let select = read::get_records(&model, columns.into_iter(), query_arguments)
            .and_where(query_builder::conditions(&relation_columns, from_record_ids));

        Query::from(select)
    } else {
        let is_with_pagination = query_arguments.is_with_pagination();
        let base = ManyRelatedRecordsBaseQuery::new(from_field, from_record_ids, query_arguments, columns);

        if is_with_pagination {
            T::with_pagination(base)
        } else {
            T::without_pagination(base)
        }
    };

    let records: crate::Result<Vec<Record>> = conn
        .filter(query, idents.as_slice())
        .await?
        .into_iter()
        .map(|mut row| {
            let relation_cols = from_field.relation_columns(true);
            let mut parent_ids: Vec<(DataSourceFieldRef, PrismaValue)> = Vec::with_capacity(relation_cols.len());

            if is_with_pagination && T::uses_row_number() {
                let _ = row.values.pop();
            }
            // Todo: This doesn't work with @relation(references ...), it assumes primary ids.
            // parent id is always the last column
            for field in from_field.linking_fields().fields() {
                match field {
                    Field::Scalar(sf) => {
                        let val = row.values.pop().ok_or(SqlError::ColumnDoesNotExist)?;
                        parent_ids.push((sf.data_source_field().clone(), val))
                    }
                    Field::Relation(rf) => {
                        for field in rf.data_source_fields().iter() {
                            let val = row.values.pop().ok_or(SqlError::ColumnDoesNotExist)?;
                            parent_ids.push((field.clone(), val))
                        }
                    }
                }
            }

            // ModelIdentifier fields are in the end, we pop them in reverse
            // order so we should flip them before returning.
            parent_ids.reverse();

            // Relation id is always the second last value. We don't need it
            // here and we don't need it in the record.
            let _ = row.values.pop();

            let mut record = Record::from(row);

            for (_, value) in parent_ids.iter() {
                record.values.push(value.clone()); // we need the id there as well for some reason :shrug:
            }

            record.set_parent_id(RecordIdentifier::from(parent_ids));

            Ok(record)
        })
        .collect();

    Ok(ManyRecords {
        records: records?,
        field_names,
    })
}

pub async fn count_by_model(
    conn: &dyn QueryExt,
    model: &ModelRef,
    query_arguments: QueryArguments,
) -> crate::Result<usize> {
    let query = read::count_by_model(model, query_arguments);
    let result = conn.find_int(query).await? as usize;

    Ok(result)
}
