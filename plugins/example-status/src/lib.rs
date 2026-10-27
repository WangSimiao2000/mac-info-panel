use extism_pdk::{FnResult, Json, plugin_fn};
use panel_model::{
    GaugeWidget, MetricWidget, PageId, PageModel, Section, StatusTone, StatusWidget, Widget,
};
use panel_plugin_api::{RefreshRequest, RefreshResponse};

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
                    value: request.platform,
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
