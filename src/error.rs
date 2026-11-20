use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    Parser,
    TypeCheck,
    Verifier,
    Inquisitor,
}

#[derive(Debug, Clone)]
pub struct CompileError {
    pub stage: Stage,
    pub file: String,
    pub line: usize,
    pub message: String,
}

impl CompileError {
    pub fn new(
        stage: Stage,
        file: impl Into<String>,
        line: usize,
        message: impl Into<String>,
    ) -> Self {
        Self {
            stage,
            file: file.into(),
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tag = match self.stage {
            Stage::Parser => "SYNTAX",
            Stage::TypeCheck => "TYPE",
            Stage::Verifier => "VETO",
            Stage::Inquisitor => "INQUISITOR",
        };
        if self.line > 0 {
            write!(
                f,
                "[{} | {}:{}]: {}",
                tag, self.file, self.line, self.message
            )
        } else {
            write!(f, "[{} | {}]: {}", tag, self.file, self.message)
        }
    }
}

pub type ErrList = Vec<CompileError>;

#[derive(Debug, Default)]
pub struct ErrorBag {
    pub errors: ErrList,
}

impl ErrorBag {
    pub fn push(&mut self, stage: Stage, file: &str, line: usize, msg: impl Into<String>) {
        self.errors.push(CompileError::new(stage, file, line, msg));
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn print_all(&self) {
        for err in &self.errors {
            eprintln!("{}", err);
        }
    }
}
