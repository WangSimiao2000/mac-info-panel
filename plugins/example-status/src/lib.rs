use extism_pdk::{FnResult, Json, plugin_fn};
use panel_model::{
    GaugeWidget, MetricWidget, PageId, PageModel, Section, StatusTone, StatusWidget, Widget,
};
use panel_plugin_api::{
    ApiVersion, MetadataRequest, MetadataResponse, PluginPageMetadata, PluginRuntimeKind,
    RefreshRequest, RefreshResponse, ValidateConfigRequest, ValidateConfigResponse,
};

fn page_metadata() -> PluginPageMetadata {
    PluginPageMetadata {
        id: "example.status".into(),
        title: "Example Status".into(),
        icon: Some("gauge".into()),
    }
}

#[plugin_fn]
pub fn metadata(Json(_request): Json<MetadataRequest>) -> FnResult<Json<MetadataResponse>> {
    Ok(Json(MetadataResponse {
        id: "io.github.wangsimiao.example-status".into(),
        version: "0.1.0".into(),
        api_version: ApiVersion { major: 1, minor: 0 },
        runtime: PluginRuntimeKind::WasmV1,
        pages: vec![page_metadata()],
    }))
}

#[plugin_fn]
pub fn validate_config(
    Json(_request): Json<ValidateConfigRequest>,
) -> FnResult<Json<ValidateConfigResponse>> {
    Ok(Json(ValidateConfigResponse {
        valid: true,
        diagnostics: Vec::new(),
    }))
}

#[plugin_fn]
pub fn refresh(Json(request): Json<RefreshRequest>) -> FnResult<Json<RefreshResponse>> {
    let page = PageModel {
        schema_version: 1,
        id: PageId::new("example.status")?,
        title: "Example Status".into(),
        icon: Some("gauge".into()),
        sections: vec![Section {
            title: Some("Plugin runtime".into()),
            widgets: vec![
                Widget::Status(StatusWidget {
                    label: "Sandbox".into(),
                    value: "Active".into(),
                    tone: StatusTone::Success,
                }),
                Widget::Metric(MetricWidget {
                    label: "Platform".into(),
                    value: request.platform.unwrap_or_else(|| "Unavailable".into()),
                    unit: None,
                }),
                Widget::Gauge(GaugeWidget {
                    label: "Example load".into(),
                    value: 0.42,
                }),
            ],
        }],
        updated_at_unix_ms: Some(request.now_unix_ms),
    };

    Ok(Json(RefreshResponse { page }))
}
