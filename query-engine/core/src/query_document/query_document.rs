//! Intermediate representation of the input document that is used by the query engine to build
//! query ASTs and validate the incoming data.
//!
//! Helps decoupling the incoming protocol layer from the query engine, i.e. allows the query engine
//! to be agnostic to the actual protocol that is used on upper layers, as long as they translate
//! to this simple intermediate representation.
//!
//! The mapping illustrated with GraphQL (GQL):
//! - There can be multiple queries and/or mutations in one GQL request, usually designated by "query / {}" or "mutation".
//! - Inside the queries / mutations are fields in GQL. In Prisma, every one of those designates exactly one `Operation` with a `Selection`.
//! - Operations are broadly divided into reading (query in GQL) or writing (mutation).
//! - The field that designates the `Operation` pretty much exactly maps to a `Selection`:
//!    - It can have arguments,
//!    - it can be aliased,
//!    - it can have a number of nested selections (selection set in GQL).
//! - Arguments contain concrete values and complex subtypes that are parsed and validated by the query builders, and then used for querying data (input types in GQL).
//!
use itertools::Itertools;
use std::collections::BTreeMap;

#[derive(Debug)]
pub enum QueryDocument {
    Single(Operation),
    Multi(BatchDocument),
}

impl QueryDocument {
    pub fn dedup_operations(self) -> Self {
        match self {
            Self::Single(operation) => Self::Single(operation.dedup_selections()),
            Self::Multi(_) => todo!(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Operation {
    Read(Selection),
    Write(Selection),
}

impl Operation {
    pub fn is_find_one(&self) -> bool {
        match self {
            Self::Read(selection) => selection.is_find_one(),
            _ => false,
        }
    }

    pub fn into_read(self) -> Option<Selection> {
        match self {
            Self::Read(sel) => Some(sel),
            _ => None,
        }
    }

    pub fn into_write(self) -> Option<Selection> {
        match self {
            Self::Write(sel) => Some(sel),
            _ => None,
        }
    }
}

impl Operation {
    pub fn dedup_selections(self) -> Self {
        match self {
            Self::Read(s) => Self::Read(s.dedup()),
            Self::Write(s) => Self::Write(s.dedup()),
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Read(s) => &s.name,
            Self::Write(s) => &s.name,
        }
    }

    fn nested_selections(&self) -> &[Selection] {
        match self {
            Self::Read(s) => &s.nested_selections,
            Self::Write(s) => &s.nested_selections,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub name: String,
    pub alias: Option<String>,
    pub arguments: Vec<(String, QueryValue)>,
    pub nested_selections: Vec<Selection>,
}

impl Selection {
    pub fn dedup(mut self) -> Self {
        self.nested_selections = self
            .nested_selections
            .into_iter()
            .unique_by(|s| s.name.clone())
            .collect();

        self
    }

    pub fn is_find_one(&self) -> bool {
        self.name.starts_with("findOne")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueryValue {
    Int(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Null,
    Enum(String),
    List(Vec<QueryValue>),
    Object(BTreeMap<String, QueryValue>),
}

impl QueryValue {
    pub fn into_object(self) -> Option<BTreeMap<String, QueryValue>> {
        match self {
            Self::Object(map) => Some(map),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum BatchDocument {
    Multi(Vec<Operation>),
    Compact(CompactedDocument),
}

impl BatchDocument {
    pub fn new(operations: Vec<Operation>) -> Self {
        Self::Multi(operations)
    }

    fn can_compact(&self) -> bool {
        match self {
            Self::Multi(operations) => match operations.split_first() {
                Some((first, rest)) if first.is_find_one() => rest.into_iter().all(|op| {
                    op.is_find_one() && first.name() == op.name() && first.nested_selections() == op.nested_selections()
                }),
                _ => false,
            },
            Self::Compact(_) => false,
        }
    }

    pub fn compact(self) -> Self {
        if self.can_compact() {
            todo!()
        } else {
            self
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompactedDocument {
    pub arguments: Vec<QueryValue>,
    pub operation: Operation,
}

impl From<Vec<Operation>> for CompactedDocument {
    fn from(ops: Vec<Operation>) -> Self {
        let selections: Vec<Selection> = ops
            .into_iter()
            .map(|op| op.into_read().expect("Trying to compact a write operation."))
            .collect();

        let selection = {
            let argument_name = selections[0].arguments[0]
                .1
                .clone()
                .into_object()
                .unwrap()
                .into_iter()
                .next()
                .unwrap()
                .0;

            let values: Vec<QueryValue> = selections
                .iter()
                .map(|selection| {
                    let obj = selection.arguments[0]
                        .1
                        .clone()
                        .into_object()
                        .expect("Trying to compact a selection with non-object argument");

                    obj.into_iter().map(|(_, val)| val).next().unwrap()
                })
                .collect();

            let mut argument = BTreeMap::new();
            argument.insert(format!("{}_in", argument_name), QueryValue::List(values));

            Selection {
                name: selections[0].name.replacen("findOne", "findMany", 1),
                alias: selections[0].alias.clone(),
                nested_selections: selections[0].nested_selections.clone(),
                arguments: vec![(String::from("where"), QueryValue::Object(argument))],
            }
        };

        let arguments = selections.into_iter().map(|mut selection| selection.arguments.pop().unwrap().1).collect();

        Self {
            arguments,
            operation: Operation::Read(selection),
        }
    }
}
