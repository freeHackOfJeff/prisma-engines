use datamodel::dml::{
    Datamodel, DefaultValue, Enum, Field, FieldArity, FieldType, IndexDefinition, Model, ScalarType, WithDatabaseName,
};

pub(crate) fn walk_models<'a>(datamodel: &'a Datamodel) -> impl Iterator<Item = ModelRef<'a>> + 'a {
    datamodel.models.iter().map(move |model| ModelRef { datamodel, model })
}

/// Iterator to walk all the fields in the schema, associating them with their parent model.
pub(super) fn walk_fields<'a>(datamodel: &'a Datamodel) -> impl Iterator<Item = FieldRef<'a>> + 'a {
    datamodel.models().flat_map(move |model| {
        model.fields().map(move |field| FieldRef {
            datamodel,
            model,
            field,
        })
    })
}

pub(crate) struct ModelRef<'a> {
    pub(crate) datamodel: &'a Datamodel,
    pub(crate) model: &'a Model,
}

impl<'a> ModelRef<'a> {
    pub(super) fn database_name(&self) -> &'a str {
        self.model.database_name.as_ref().unwrap_or(&self.model.name)
    }

    pub(super) fn db_name(&self) -> &str {
        self.model.single_database_name().unwrap_or_else(|| &self.model.name)
    }

    pub(super) fn fields<'b>(&'b self) -> impl Iterator<Item = FieldRef<'a>> + 'b {
        self.model.fields().map(move |field| FieldRef {
            datamodel: self.datamodel,
            model: self.model,
            field,
        })
    }

    pub(super) fn find_field(&self, name: &str) -> Option<FieldRef<'a>> {
        self.model
            .fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| FieldRef {
                datamodel: self.datamodel,
                field,
                model: self.model,
            })
    }

    pub(super) fn indexes<'b>(&'b self) -> impl Iterator<Item = &'a IndexDefinition> + 'b {
        self.model.indices.iter()
    }

    pub(super) fn model(&self) -> &'a Model {
        self.model
    }

    pub(super) fn name(&self) -> &'a str {
        &self.model.name
    }

    pub(super) fn id_fields<'b>(&'b self) -> impl Iterator<Item = FieldRef<'a>> + 'b {
        // Single-id models
        self.model
            .fields()
            .filter(|field| field.is_id)
            // Compound id models
            .chain(
                self.model
                    .id_fields
                    .iter()
                    .filter_map(move |field_name| self.model.fields().find(|field| field.name.as_str() == field_name)),
            )
            .map(move |field| FieldRef {
                datamodel: self.datamodel,
                model: self.model,
                field,
            })
    }
}

pub(super) struct FieldRef<'a> {
    datamodel: &'a Datamodel,
    model: &'a Model,
    field: &'a Field,
}

impl<'a> FieldRef<'a> {
    pub(super) fn arity(&self) -> FieldArity {
        self.field.arity
    }

    pub(super) fn db_name(&self) -> &'a str {
        self.field.single_database_name().unwrap_or(self.name())
    }

    pub(super) fn default_value(&self) -> Option<&'a DefaultValue> {
        self.field.default_value.as_ref()
    }

    pub(super) fn field_type(&self) -> TypeRef<'a> {
        match &self.field.field_type {
            FieldType::Enum(name) => TypeRef::Enum(EnumRef {
                datamodel: self.datamodel,
                r#enum: self.datamodel.find_enum(name).unwrap(),
            }),
            FieldType::Base(scalar_type) => TypeRef::Base(*scalar_type),
            _ => TypeRef::Other,
        }
    }

    pub(super) fn is_unique(&self) -> bool {
        self.field.is_unique
    }

    pub(super) fn is_id(&self) -> bool {
        self.field.is_id
    }

    pub(super) fn model(&self) -> ModelRef<'a> {
        ModelRef {
            model: self.model,
            datamodel: self.datamodel,
        }
    }

    pub(super) fn name(&self) -> &'a str {
        &self.field.name
    }
}

#[derive(Debug)]
pub(super) enum TypeRef<'a> {
    Enum(EnumRef<'a>),
    Base(ScalarType),
    Other,
}

impl<'a> TypeRef<'a> {
    pub(super) fn as_enum(&self) -> Option<EnumRef<'a>> {
        match self {
            TypeRef::Enum(r) => Some(*r),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct EnumRef<'a> {
    pub(super) r#enum: &'a Enum,
    datamodel: &'a Datamodel,
}

impl<'a> EnumRef<'a> {
    pub(super) fn name(&self) -> &'a str {
        &self.r#enum.name
    }

    pub(super) fn values(&self) -> &'a [String] {
        &self.r#enum.values
    }

    pub(super) fn db_name(&self) -> &'a str {
        self.r#enum.single_database_name().unwrap_or(&self.r#enum.name)
    }
}
