use std::time::Duration;

use panel_plugin_api::{PluginManifest, RefreshRequest, RefreshResponse};
use panel_plugin_runtime_extism::{PluginInstance, PluginRuntime, RuntimeError};
use thiserror::Error;

pub struct PluginHost<R: PluginRuntime> {
    runtime: R,
    timeout: Duration,
}

impl<R: PluginRuntime> PluginHost<R> {
    pub fn new(runtime: R, timeout: Duration) -> Self {
        Self { runtime, timeout }
    }

    pub fn load(
        &self,
        manifest: PluginManifest,
        wasm: &[u8],
    ) -> Result<LoadedPlugin<R::Instance>, HostError> {
        manifest.validate()?;
        let instance = self.runtime.load(wasm, self.timeout)?;
        Ok(LoadedPlugin { manifest, instance })
    }
}

pub struct LoadedPlugin<I: PluginInstance> {
    manifest: PluginManifest,
    instance: I,
}

impl<I: PluginInstance> LoadedPlugin<I> {
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub fn refresh(&mut self, request: &RefreshRequest) -> Result<RefreshResponse, HostError> {
        let input = serde_json::to_vec(request)?;
        let output = self.instance.call("refresh", &input)?;
        let response: RefreshResponse = serde_json::from_slice(&output)?;
        response.page.validate()?;
        Ok(response)
    }
}

#[derive(Debug, Error)]
pub enum HostError {
    #[error(transparent)]
    Manifest(#[from] panel_plugin_api::ManifestError),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Model(#[from] panel_model::ModelError),
}
