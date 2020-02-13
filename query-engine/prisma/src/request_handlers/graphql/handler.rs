use super::protocol_adapter::GraphQLProtocolAdapter;
use crate::{context::PrismaContext, PrismaError, PrismaRequest, PrismaResponse, PrismaResult, RequestHandler};
use async_trait::async_trait;
use futures::{future, FutureExt};
use graphql_parser as gql;
use query_core::{response_ir, BatchDocument, CoreError, Operation, QueryDocument};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, convert::TryFrom, panic::AssertUnwindSafe, sync::Arc};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SingleQuery {
    query: String,
    operation_name: Option<String>,
    variables: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiQuery {
    batch: Vec<SingleQuery>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum GraphQlBody {
    Single(SingleQuery),
    Multi(MultiQuery),
}

impl TryFrom<GraphQlBody> for QueryDocument {
    type Error = PrismaError;

    fn try_from(body: GraphQlBody) -> PrismaResult<Self> {
        match body {
            GraphQlBody::Single(body) => {
                let gql_doc = gql::parse_query(&body.query)?;
                let operation = GraphQLProtocolAdapter::convert(gql_doc, body.operation_name)?;

                Ok(QueryDocument::Single(operation))
            }
            GraphQlBody::Multi(bodies) => {
                let operations: PrismaResult<Vec<Operation>> = bodies
                    .batch
                    .into_iter()
                    .map(|body| {
                        let gql_doc = gql::parse_query(&body.query)?;
                        GraphQLProtocolAdapter::convert(gql_doc, body.operation_name)
                    })
                    .collect();

                Ok(QueryDocument::Multi(BatchDocument::new(operations?)))
            }
        }
    }
}

pub struct GraphQlRequestHandler;

#[allow(unused_variables)]
#[async_trait]
impl RequestHandler for GraphQlRequestHandler {
    type Body = GraphQlBody;

    async fn handle<S>(&self, req: S, ctx: &Arc<PrismaContext>) -> PrismaResponse
    where
        S: Into<PrismaRequest<Self::Body>> + Send + Sync + 'static,
    {
        let request = req.into();

        match QueryDocument::try_from(request.body) {
            Ok(QueryDocument::Single(query)) => handle_single_query(dbg!(query), ctx.clone()).await,
            Ok(QueryDocument::Multi(batch)) => match batch.compact() {
                BatchDocument::Multi(batch) => handle_batch(batch, ctx).await,
                BatchDocument::Compact(_compacted) => todo!(),
            }
            Err(err) => {
                let mut responses = response_ir::Responses::default();
                responses.insert_error(err);

                PrismaResponse::Single(responses)
            }
        }
    }
}

async fn handle_single_query(query: Operation, ctx: Arc<PrismaContext>) -> PrismaResponse {
    use user_facing_errors::Error;

    let responses = match AssertUnwindSafe(handle_graphql_query(query, &*ctx))
        .catch_unwind()
        .await
    {
        Ok(Ok(responses)) => responses,
        Ok(Err(err)) => {
            let mut responses = response_ir::Responses::default();
            responses.insert_error(err);
            responses
        }
        // panicked
        Err(err) => {
            let mut responses = response_ir::Responses::default();
            let error = Error::from_panic_payload(&err);

            responses.insert_error(error);
            responses
        }
    };

    PrismaResponse::Single(responses)
}

async fn handle_batch(queries: Vec<Operation>, ctx: &Arc<PrismaContext>) -> PrismaResponse {
    let mut futures = Vec::with_capacity(queries.len());

    for operation in queries.into_iter() {
        futures.push(tokio::spawn(handle_single_query(operation, ctx.clone())));
    }

    let responses = future::join_all(futures)
        .await
        .into_iter()
        .map(|res| res.expect("IO Error in tokio::spawn"))
        .collect();

    PrismaResponse::Multi(responses)
}

async fn handle_graphql_query(query_doc: Operation, ctx: &PrismaContext) -> PrismaResult<response_ir::Responses> {
    ctx.executor
        .execute(query_doc, Arc::clone(ctx.query_schema()))
        .await
        .map_err(|err| {
            debug!("{}", err);
            let ce: CoreError = err.into();
            ce.into()
        })
}
