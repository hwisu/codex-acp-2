use super::*;

pub(in crate::thread::tests) struct StubAuth;

impl Auth for StubAuth {
    async fn logout(&self) -> Result<bool, Error> {
        Ok(true)
    }
}

pub(in crate::thread::tests) struct StubModelsManager;

impl ModelsManagerImpl for StubModelsManager {
    fn get_model(
        &self,
        _model_id: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = String> + Send + '_>> {
        Box::pin(async { all_model_presets()[0].clone().id })
    }

    fn list_models(&self) -> Pin<Box<dyn Future<Output = Vec<ModelPreset>> + Send + '_>> {
        Box::pin(async { all_model_presets().to_owned() })
    }
}
