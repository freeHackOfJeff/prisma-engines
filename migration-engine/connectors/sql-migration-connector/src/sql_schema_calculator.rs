use crate::{sql_renderer::IteratorJoin, DatabaseInfo, SqlResult};
use chrono::*;
use datamodel::common::*;
use datamodel::*;
use prisma_models::{DatamodelConverter, TempManifestationHolder, TempRelationHolder};
use quaint::prelude::SqlFamily;
use sql_schema_describer::{self as sql, ColumnArity};

pub struct SqlSchemaCalculator<'a> {
    data_model: &'a Datamodel,
    database_info: &'a DatabaseInfo,
}

impl<'a> SqlSchemaCalculator<'a> {
    pub fn calculate(data_model: &Datamodel, database_info: &DatabaseInfo) -> SqlResult<sql::SqlSchema> {
        let calculator = SqlSchemaCalculator {
            data_model,
            database_info,
        };
        calculator.calculate_internal()
    }

    fn calculate_internal(&self) -> SqlResult<sql::SqlSchema> {
        let mut tables = Vec::new();
        let model_tables_without_inline_relations = self.calculate_model_tables()?;
        let mut model_tables = self.add_inline_relations_to_model_tables(model_tables_without_inline_relations)?;
        let mut relation_tables = self.calculate_relation_tables()?;

        tables.append(&mut model_tables);
        tables.append(&mut relation_tables);

        // guarantee same sorting as in the sql-schema-describer
        for table in &mut tables {
            table
                .columns
                .sort_unstable_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        }

        let enums = self.calculate_enums();
        let sequences = Vec::new();

        Ok(sql::SqlSchema {
            tables,
            enums,
            sequences,
        })
    }

    fn calculate_enums(&self) -> Vec<sql::Enum> {
        self.data_model
            .enums()
            .map(|r#enum| sql::Enum {
                name: r#enum
                    .single_database_name()
                    .map(|s| s.to_owned())
                    .unwrap_or_else(|| r#enum.name.clone()),
                values: r#enum.values.clone(),
            })
            .collect()
    }

    fn calculate_model_tables(&self) -> SqlResult<Vec<ModelTable>> {
        self.data_model
            .models()
            .map(|model| {
                let columns = model
                    .fields()
                    .flat_map(|f| match &f.field_type {
                        FieldType::Base(_) => Some(sql::Column {
                            name: f.db_name(),
                            tpe: column_type(f),
                            default: f.migration_value_new(&self.data_model),
                            auto_increment: {
                                match &f.default_value {
                                    Some(DefaultValue::Expression(ValueGenerator {
                                        name: _,
                                        args: _,
                                        generator: ValueGeneratorFn::Autoincrement,
                                    })) => true,
                                    _ => false,
                                }
                            },
                        }),
                        FieldType::Enum(r#enum) => {
                            let enum_db_name = self
                                .data_model
                                .find_enum(r#enum)
                                .unwrap()
                                .single_database_name()
                                .unwrap_or_else(|| r#enum.as_str());
                            Some(sql::Column {
                                name: f.db_name(),
                                tpe: enum_column_type(f, &self.database_info, enum_db_name),
                                default: f.migration_value_new(&self.data_model),
                                auto_increment: false,
                            })
                        }
                        _ => None,
                    })
                    .collect();

                let primary_key = sql::PrimaryKey {
                    columns: id_fields(model).map(|field| field.db_name()).collect(),
                    sequence: None,
                };

                let single_field_indexes = model.fields().filter_map(|f| {
                    if f.is_unique {
                        Some(sql::Index {
                            name: format!("{}.{}", &model.db_name(), &f.db_name()),
                            columns: vec![f.db_name().clone()],
                            tpe: sql::IndexType::Unique,
                        })
                    } else {
                        None
                    }
                });

                let multiple_field_indexes = model.indices.iter().map(|index_definition: &IndexDefinition| {
                    let referenced_fields: Vec<&Field> = index_definition
                        .fields
                        .iter()
                        .map(|field_name| model.find_field(field_name).expect("Unknown field in index directive."))
                        .collect();

                    sql::Index {
                        name: index_definition.name.clone().unwrap_or_else(|| {
                            format!(
                                "{}.{}",
                                &model.db_name(),
                                referenced_fields.iter().map(|field| field.db_name()).join("_")
                            )
                        }),
                        // The model index definition uses the model field names, but the SQL Index
                        // wants the column names.
                        columns: referenced_fields.iter().map(|field| field.db_name()).collect(),
                        tpe: if index_definition.tpe == IndexType::Unique {
                            sql::IndexType::Unique
                        } else {
                            sql::IndexType::Normal
                        },
                    }
                });

                let table = sql::Table {
                    name: model.db_name().to_owned(),
                    columns,
                    indices: single_field_indexes.chain(multiple_field_indexes).collect(),
                    primary_key: Some(primary_key),
                    foreign_keys: Vec::new(),
                };

                Ok(ModelTable {
                    model: model.clone(),
                    table,
                })
            })
            .collect()
    }

    fn add_inline_relations_to_model_tables(&self, model_tables: Vec<ModelTable>) -> SqlResult<Vec<sql::Table>> {
        let mut result = Vec::new();
        let relations = self.calculate_relations();
        for mut model_table in model_tables {
            for relation in relations.iter() {
                match &relation.manifestation {
                    TempManifestationHolder::Inline {
                        in_table_of_model,
                        column: column_name,
                        referenced_fields,
                    } if in_table_of_model == &model_table.model.name => {
                        let (model, related_model) = if model_table.model == relation.model_a {
                            (&relation.model_a, &relation.model_b)
                        } else {
                            (&relation.model_b, &relation.model_a)
                        };

                        let field = model.fields().find(|f| &f.db_name() == column_name).unwrap();

                        let referenced_fields: Vec<&Field> = if referenced_fields.is_empty() {
                            // TODO: should this the function unique_fields instead?
                            id_fields(related_model).collect()
                        } else {
                            let fields: Vec<_> = related_model
                                .fields()
                                .filter(|field| {
                                    referenced_fields
                                        .iter()
                                        .any(|referenced| referenced.as_str() == field.name)
                                })
                                .collect();

                            if fields.len() != referenced_fields.len() {
                                return Err(crate::SqlError::Generic(anyhow::anyhow!(
                                    "References to unknown fields {referenced_fields:?} on `{model_name}`",
                                    model_name = related_model.name,
                                    referenced_fields = referenced_fields,
                                )));
                            }

                            fields
                        };

                        let columns: Vec<sql::Column> = if referenced_fields.len() == 1 {
                            let referenced_field = referenced_fields.iter().next().unwrap();

                            vec![sql::Column {
                                name: column_name.clone(),
                                tpe: column_type_for_scalar_type(
                                    scalar_type_for_field(referenced_field),
                                    column_arity(&field),
                                ),
                                default: None,
                                auto_increment: false,
                            }]
                        } else {
                            referenced_fields
                                .iter()
                                .map(|referenced_field| sql::Column {
                                    name: format!("{}_{}", column_name, referenced_field.db_name()),
                                    tpe: column_type_for_scalar_type(
                                        scalar_type_for_field(referenced_field),
                                        column_arity(&field),
                                    ),
                                    default: None,
                                    auto_increment: false,
                                })
                                .collect()
                        };

                        let foreign_key = sql::ForeignKey {
                            constraint_name: None,
                            columns: columns.iter().map(|col| col.name.to_owned()).collect(),
                            referenced_table: related_model.db_name().to_owned(),
                            referenced_columns: referenced_fields
                                .iter()
                                .map(|referenced_field| referenced_field.db_name())
                                .collect(),
                            on_delete_action: match column_arity(&field) {
                                ColumnArity::Required => sql::ForeignKeyAction::Restrict,
                                _ => sql::ForeignKeyAction::SetNull,
                            },
                        };

                        model_table.table.columns.extend(columns);
                        model_table.table.foreign_keys.push(foreign_key);

                        if relation.is_one_to_one() {
                            add_one_to_one_relation_unique_index(&mut model_table.table, column_name)
                        }
                    }
                    _ => {}
                }
            }
            result.push(model_table.table);
        }
        Ok(result)
    }

    fn calculate_relation_tables(&self) -> SqlResult<Vec<sql::Table>> {
        let mut result = Vec::new();
        for relation in self.calculate_relations().iter() {
            match &relation.manifestation {
                TempManifestationHolder::Table => {
                    let a_columns = relation_table_columns(&relation.model_a, relation.model_a_column());
                    let mut b_columns = relation_table_columns(&relation.model_b, relation.model_b_column());

                    let foreign_keys = vec![
                        sql::ForeignKey {
                            constraint_name: None,
                            columns: a_columns.iter().map(|col| col.name.clone()).collect(),
                            referenced_table: relation.model_a.db_name().to_owned(),
                            referenced_columns: unique_criteria(&relation.model_a)
                                .map(|field| field.db_name())
                                .collect(),
                            on_delete_action: sql::ForeignKeyAction::Cascade,
                        },
                        sql::ForeignKey {
                            constraint_name: None,
                            columns: b_columns.iter().map(|col| col.name.clone()).collect(),
                            referenced_table: relation.model_b.db_name().to_owned(),
                            referenced_columns: unique_criteria(&relation.model_b)
                                .map(|field| field.db_name())
                                .collect(),
                            on_delete_action: sql::ForeignKeyAction::Cascade,
                        },
                    ];

                    let mut columns = a_columns;
                    columns.append(&mut b_columns);

                    let index = sql::Index {
                        // TODO: rename
                        name: format!("{}_AB_unique", relation.table_name()),
                        columns: columns.iter().map(|col| col.name.clone()).collect(),
                        tpe: sql::IndexType::Unique,
                    };

                    let table = sql::Table {
                        name: relation.table_name(),
                        columns,
                        indices: vec![index],
                        primary_key: None,
                        foreign_keys,
                    };
                    result.push(table);
                }
                _ => {}
            }
        }
        Ok(result)
    }

    fn calculate_relations(&self) -> Vec<TempRelationHolder> {
        DatamodelConverter::calculate_relations(&self.data_model)
    }
}

fn relation_table_columns(referenced_model: &Model, reference_field_name: String) -> Vec<sql::Column> {
    // TODO: must also work with multi field unique
    if referenced_model.id_fields.is_empty() {
        let unique_field = referenced_model.fields().find(|f| f.is_unique);
        let id_field = referenced_model.fields().find(|f| f.is_id());

        let unique_field = id_field
            .or(unique_field)
            .expect(&format!("No unique criteria found in model {}", &referenced_model.name));

        vec![sql::Column {
            name: reference_field_name,
            tpe: column_type(unique_field),
            default: None,
            auto_increment: false,
        }]
    } else {
        id_fields(referenced_model)
            .map(|referenced_field| sql::Column {
                name: format!(
                    "{reference_field_name}_{referenced_column_name}",
                    reference_field_name = reference_field_name,
                    referenced_column_name = referenced_field.db_name()
                ),
                tpe: column_type(referenced_field),
                default: None,
                auto_increment: false,
            })
            .collect()
    }
}

#[derive(PartialEq, Debug)]
struct ModelTable {
    table: sql::Table,
    model: Model,
}

pub trait ModelExtensions {
    fn db_name(&self) -> &str;
}

impl ModelExtensions for Model {
    fn db_name(&self) -> &str {
        self.single_database_name().unwrap_or_else(|| &self.name)
    }
}

pub trait FieldExtensions {
    fn is_id(&self) -> bool;

    fn is_list(&self) -> bool;

    fn db_name(&self) -> String;

    fn migration_value(&self, datamodel: &Datamodel) -> ScalarValue;

    fn migration_value_new(&self, datamodel: &Datamodel) -> Option<String>;
}

impl FieldExtensions for Field {
    fn is_id(&self) -> bool {
        self.is_id
    }

    fn is_list(&self) -> bool {
        self.arity == FieldArity::List
    }

    fn db_name(&self) -> String {
        self.single_database_name().unwrap_or(&self.name).to_string()
    }

    fn migration_value(&self, datamodel: &Datamodel) -> ScalarValue {
        self.default_value
            .clone()
            .and_then(|df| df.get())
            .unwrap_or_else(|| default_migration_value(&self.field_type, datamodel))
    }

    fn migration_value_new(&self, datamodel: &Datamodel) -> Option<String> {
        let value = match (&self.default_value, self.arity) {
            (Some(df), _) => match df {
                dml::DefaultValue::Single(s) => s.clone(),
                dml::DefaultValue::Expression(_) => default_migration_value(&self.field_type, datamodel),
            },
            // This is a temporary hack until we can report impossible unexecutable migrations.
            (None, FieldArity::Required) => default_migration_value(&self.field_type, datamodel),
            (None, _) => return None,
        };

        let result = match value {
            ScalarValue::Boolean(x) => {
                if x {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            ScalarValue::Int(x) => format!("{}", x),
            ScalarValue::Float(x) => format!("{}", x),
            ScalarValue::Decimal(x) => format!("{}", x),
            ScalarValue::String(x) => format!("{}", x),

            ScalarValue::DateTime(x) => {
                let mut raw = format!("{}", x); // this will produce a String 1970-01-01 00:00:00 UTC
                raw.truncate(raw.len() - 4); // strip the UTC suffix
                format!("{}", raw)
            }
            ScalarValue::ConstantLiteral(x) => format!("{}", x), // this represents enum values
        };

        if self.is_id() {
            None
        } else {
            Some(result)
        }
    }
}

fn default_migration_value(field_type: &FieldType, datamodel: &Datamodel) -> ScalarValue {
    match field_type {
        FieldType::Base(ScalarType::Boolean) => ScalarValue::Boolean(false),
        FieldType::Base(ScalarType::Int) => ScalarValue::Int(0),
        FieldType::Base(ScalarType::Float) => ScalarValue::Float(0.0),
        FieldType::Base(ScalarType::String) => ScalarValue::String("".to_string()),
        FieldType::Base(ScalarType::Decimal) => ScalarValue::Decimal(0.0),
        FieldType::Base(ScalarType::DateTime) => {
            let naive = NaiveDateTime::from_timestamp(0, 0);
            let datetime: DateTime<Utc> = DateTime::from_utc(naive, Utc);
            ScalarValue::DateTime(datetime)
        }
        FieldType::Enum(ref enum_name) => {
            let inum = datamodel
                .find_enum(&enum_name)
                .expect(&format!("Enum {} was not present in the Datamodel.", enum_name));
            let first_value = inum
                .values
                .first()
                .expect(&format!("Enum {} did not contain any values.", enum_name));
            ScalarValue::String(first_value.to_string())
        }
        _ => unimplemented!("this functions must only be called for scalar fields"),
    }
}

fn enum_column_type(field: &Field, database_info: &DatabaseInfo, db_name: &str) -> sql::ColumnType {
    let arity = column_arity(field);
    match database_info.sql_family() {
        SqlFamily::Postgres | SqlFamily::Mysql => {
            sql::ColumnType::pure(sql::ColumnTypeFamily::Enum(db_name.to_owned()), arity)
        }
        _ => column_type(field),
    }
}

fn column_type(field: &Field) -> sql::ColumnType {
    column_type_for_scalar_type(scalar_type_for_field(field), column_arity(field))
}

fn scalar_type_for_field(field: &Field) -> &ScalarType {
    match &field.field_type {
        FieldType::Base(ref scalar) => &scalar,
        FieldType::Enum(_) => &ScalarType::String,
        x => panic!(format!(
            "This field type is not suported here. Field type is {:?} on field {}",
            x, field.name
        )),
    }
}

fn column_arity(field: &Field) -> sql::ColumnArity {
    match &field.arity {
        FieldArity::Required => sql::ColumnArity::Required,
        FieldArity::List => sql::ColumnArity::List,
        FieldArity::Optional => sql::ColumnArity::Nullable,
    }
}

fn column_type_for_scalar_type(scalar_type: &ScalarType, column_arity: ColumnArity) -> sql::ColumnType {
    match scalar_type {
        ScalarType::Int => sql::ColumnType::pure(sql::ColumnTypeFamily::Int, column_arity),
        ScalarType::Float => sql::ColumnType::pure(sql::ColumnTypeFamily::Float, column_arity),
        ScalarType::Boolean => sql::ColumnType::pure(sql::ColumnTypeFamily::Boolean, column_arity),
        ScalarType::String => sql::ColumnType::pure(sql::ColumnTypeFamily::String, column_arity),
        ScalarType::DateTime => sql::ColumnType::pure(sql::ColumnTypeFamily::DateTime, column_arity),
        ScalarType::Decimal => unimplemented!(),
    }
}

fn add_one_to_one_relation_unique_index(table: &mut sql::Table, column_name: &str) {
    let index = sql::Index {
        name: format!("{}_{}", table.name, column_name),
        columns: vec![column_name.to_string()],
        tpe: sql::IndexType::Unique,
    };

    table.indices.push(index);
}

fn unique_criteria(model: &Model) -> impl Iterator<Item = &Field> {
    // TODO: the logic for order of precedence is duplicated in `relation_table_columns`
    let id_fields: Vec<&Field> = id_fields(&model).collect();
    let unique_fields: Vec<&Field> = unique_fields(&model).collect();

    if !id_fields.is_empty() {
        id_fields.into_iter()
    } else {
        unique_fields.into_iter()
    }
}

fn unique_fields(model: &Model) -> impl Iterator<Item = &Field> {
    // TODO: handle `@@unique`
    model.fields().filter(|field| field.is_unique)
}

fn id_fields(model: &Model) -> impl Iterator<Item = &Field> {
    // Single-id models
    model
        .fields()
        .filter(|field| field.is_id())
        // Compound id models
        .chain(
            model
                .id_fields
                .iter()
                .filter_map(move |field_name| model.fields().find(|field| field.name.as_str() == field_name)),
        )
}
