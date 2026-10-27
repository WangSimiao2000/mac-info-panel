use std::time::Duration;

use extism::{Manifest, Plugin, Wasm};
use thiserror::Error;

pub trait PluginInstance {
    fn call(&mut self, function: &str, input: &[u8]) -> Result<Vec<u8>, RuntimeError>;
}

pub trait PluginRuntime {
    type Instance: PluginInstance;

    fn load(&self, wasm: &[u8], timeout: Duration) -> Result<Self::Instance, RuntimeError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ExtismRuntime;

pub struct ExtismPlugin {
    plugin: Plugin,
}

impl PluginRuntime for ExtismRuntime {
    type Instance = ExtismPlugin;

    fn load(&self, wasm: &[u8], timeout: Duration) -> Result<Self::Instance, RuntimeError> {
        if wasm.is_empty() {
            return Err(RuntimeError::EmptyModule);
        }
        let manifest = Manifest::new([Wasm::data(wasm)]).with_timeout(timeout);
        let plugin = Plugin::new(manifest, [], false)
            .map_err(|error| RuntimeError::Load(error.to_string()))?;
        Ok(ExtismPlugin { plugin })
    }
}

impl PluginInstance for ExtismPlugin {
    fn call(&mut self, function: &str, input: &[u8]) -> Result<Vec<u8>, RuntimeError> {
        let output = self
            .plugin
            .call::<&[u8], &[u8]>(function, input)
            .map_err(|error| RuntimeError::Call(error.to_string()))?;
        Ok(output.to_vec())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RuntimeError {
    #[error("plugin module is empty")]
    EmptyModule,
    #[error("failed to load plugin: {0}")]
    Load(String),
    #[error("plugin call failed: {0}")]
    Call(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_modules_before_runtime_initialization() {
        assert!(matches!(
            ExtismRuntime.load(&[], Duration::from_secs(1)),
            Err(RuntimeError::EmptyModule)
        ));
    }
}
