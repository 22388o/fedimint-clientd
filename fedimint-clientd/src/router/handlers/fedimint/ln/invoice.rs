use anyhow::anyhow;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use fedimint_client::ClientHandleArc;
use fedimint_core::config::FederationId;
use fedimint_core::core::OperationId;
use fedimint_core::Amount;
use fedimint_ln_client::LightningClientModule;
use lightning_invoice::{Bolt11InvoiceDescription, Description};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::error;

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LnInvoiceRequest {
    pub amount_msat: Amount,
    pub description: String,
    pub expiry_time: Option<u64>,
    pub federation_id: FederationId,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LnInvoiceResponse {
    pub operation_id: OperationId,
    pub invoice: String,
}

async fn _invoice(
    client: ClientHandleArc,
    req: LnInvoiceRequest,
) -> Result<LnInvoiceResponse, AppError> {
    let lightning_module = client.get_first_module::<LightningClientModule>();
    let gateway_id = match lightning_module.list_gateways().await.first() {
        Some(gateway_announcement) => gateway_announcement.info.gateway_id,
        None => {
            error!("No gateways available");
            return Err(AppError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                anyhow!("No gateways available"),
            ))
        }
    };
    let gateway = lightning_module.select_gateway(&gateway_id).await.ok_or_else(|| {
        error!("Failed to select gateway");
        AppError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            anyhow!("Failed to select gateway"),
        )
    })?;

    let (operation_id, invoice, _) = lightning_module
        .create_bolt11_invoice(
            req.amount_msat,
            Bolt11InvoiceDescription::Direct(&Description::new(req.description)?),
            req.expiry_time,
            (),
            Some(gateway),
        )
        .await?;
    Ok(LnInvoiceResponse {
        operation_id,
        invoice: invoice.to_string(),
    })
}

pub async fn handle_ws(state: AppState, v: Value) -> Result<Value, AppError> {
    let v = serde_json::from_value::<LnInvoiceRequest>(v)
        .map_err(|e| AppError::new(StatusCode::BAD_REQUEST, anyhow!("Invalid request: {}", e)))?;
    let client = state.get_client(v.federation_id).await?;
    let invoice = _invoice(client, v).await?;
    let invoice_json = json!(invoice);
    Ok(invoice_json)
}

#[axum_macros::debug_handler]
pub async fn handle_rest(
    State(state): State<AppState>,
    Json(req): Json<LnInvoiceRequest>,
) -> Result<Json<LnInvoiceResponse>, AppError> {
    let client = state.get_client(req.federation_id).await?;
    let invoice = _invoice(client, req).await?;
    Ok(Json(invoice))
}
