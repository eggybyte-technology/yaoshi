use std::fmt;

pub type YaoshiResult<T> = Result<T, YaoshiError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitKind {
    Ok,
    Usage,
    Config,
    Environment,
    Build,
    Image,
    Publish,
    Internal,
}

impl ExitKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Usage => "USAGE",
            Self::Config => "CONFIG",
            Self::Environment => "ENVIRONMENT",
            Self::Build => "BUILD",
            Self::Image => "IMAGE",
            Self::Publish => "PUBLISH",
            Self::Internal => "INTERNAL",
        }
    }

    pub const fn code(self) -> i32 {
        match self {
            Self::Ok => 0,
            Self::Usage => 2,
            Self::Config => 10,
            Self::Environment => 20,
            Self::Build => 40,
            Self::Image => 50,
            Self::Publish => 60,
            Self::Internal => 100,
        }
    }
}

#[derive(Debug)]
pub struct YaoshiError {
    kind: ExitKind,
    message: String,
}

impl YaoshiError {
    pub fn new(kind: ExitKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn kind(&self) -> ExitKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(ExitKind::Usage, message)
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::new(ExitKind::Config, message)
    }

    pub fn environment(message: impl Into<String>) -> Self {
        Self::new(ExitKind::Environment, message)
    }

    pub fn build(message: impl Into<String>) -> Self {
        Self::new(ExitKind::Build, message)
    }

    pub fn image(message: impl Into<String>) -> Self {
        Self::new(ExitKind::Image, message)
    }

    pub fn publish(message: impl Into<String>) -> Self {
        Self::new(ExitKind::Publish, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ExitKind::Internal, message)
    }
}

impl fmt::Display for YaoshiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "error[{}:{}]: {}",
            self.kind.name(),
            self.kind.code(),
            self.message
        )
    }
}

impl std::error::Error for YaoshiError {}
