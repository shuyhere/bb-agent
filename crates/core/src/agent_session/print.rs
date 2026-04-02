use std::future::Future;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrintTurnStopReason {
    Completed,
    Error,
    Aborted,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintTurnResult {
    pub text: String,
    pub stop_reason: PrintTurnStopReason,
    pub error_message: Option<String>,
}

impl PrintTurnResult {
    pub fn is_error(&self) -> bool {
        matches!(
            self.stop_reason,
            PrintTurnStopReason::Error | PrintTurnStopReason::Aborted
        )
    }
}

/// Thin single-shot adapter that mirrors pi's print-mode layering:
/// the CLI owns I/O, while prompt sequencing is owned by core session code.
pub struct ThinPrintSession<F> {
    run_turn: F,
    last_result: Option<PrintTurnResult>,
}

impl<F> ThinPrintSession<F> {
    pub fn new(run_turn: F) -> Self {
        Self {
            run_turn,
            last_result: None,
        }
    }

    pub fn last_result(&self) -> Option<&PrintTurnResult> {
        self.last_result.as_ref()
    }

    pub async fn prompt<Fut, E>(&mut self, text: impl Into<String>) -> Result<&PrintTurnResult, E>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<PrintTurnResult, E>>,
    {
        let result = (self.run_turn)(text.into()).await?;
        self.last_result = Some(result);
        Ok(self
            .last_result
            .as_ref()
            .expect("thin print session stores the last turn result"))
    }

    pub async fn run<Fut, E>(
        &mut self,
        initial_message: Option<String>,
        messages: Vec<String>,
    ) -> Result<Option<&PrintTurnResult>, E>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<PrintTurnResult, E>>,
    {
        if let Some(initial_message) = initial_message {
            self.prompt(initial_message).await?;
        }

        for message in messages {
            self.prompt(message).await?;
        }

        Ok(self.last_result())
    }
}
