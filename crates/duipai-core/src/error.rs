use std::fmt;

/// DSL 错误：带可选行号（供前端定位错误行），消息为中文。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DslError {
    pub line: Option<usize>,
    pub message: String,
}

impl DslError {
    /// 不带行号的错误（表达式求值等）。
    pub fn bare(message: impl Into<String>) -> Self {
        Self { line: None, message: message.into() }
    }

    /// 带 1 起行号的错误。
    pub fn at(line: usize, message: impl Into<String>) -> Self {
        Self { line: Some(line), message: message.into() }
    }

    /// 为既有错误补上行号（若尚未携带）。
    pub fn with_line(mut self, line: usize) -> Self {
        if self.line.is_none() {
            self.line = Some(line);
        }
        self
    }
}

impl fmt::Display for DslError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "第 {} 行：{}", line, self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for DslError {}

pub type DslResult<T> = Result<T, DslError>;
