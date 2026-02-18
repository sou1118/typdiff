pub mod diff;
pub mod parse;
pub mod render;

/// A block-level element extracted from a Typst document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph {
        source_text: String,
    },
    Heading {
        depth: usize,
        body_text: String,
    },
    ListItem {
        body_text: String,
    },
    EnumItem {
        number: Option<usize>,
        body_text: String,
    },
    TermItem {
        term: String,
        description: String,
    },
    /// Atomic: not diffed internally.
    RawBlock {
        content: String,
    },
    /// Atomic: not diffed internally.
    Equation {
        block: bool,
        content: String,
    },
    /// Atomic: not diffed internally.
    FuncCall {
        content: String,
    },
    Parbreak,
}

impl Block {
    /// Returns a signature string used for block-level alignment.
    pub fn signature(&self) -> String {
        match self {
            Block::Paragraph { source_text } => format!("P:{source_text}"),
            Block::Heading { depth, body_text } => format!("H{depth}:{body_text}"),
            Block::ListItem { body_text } => format!("LI:{body_text}"),
            Block::EnumItem { number, body_text } => {
                format!(
                    "EI{}:{body_text}",
                    number.map_or("?".into(), |n| n.to_string())
                )
            }
            Block::TermItem { term, description } => format!("TI:{term}:{description}"),
            Block::RawBlock { content } => format!("RAW:{content}"),
            Block::Equation { block, content } => format!("EQ{block}:{content}"),
            Block::FuncCall { content } => format!("FC:{content}"),
            Block::Parbreak => "PB".into(),
        }
    }

    /// Returns the `BlockKind` of this block.
    pub fn kind(&self) -> BlockKind {
        match self {
            Block::Paragraph { .. } => BlockKind::Paragraph,
            Block::Heading { depth, .. } => BlockKind::Heading { depth: *depth },
            Block::ListItem { .. } => BlockKind::ListItem,
            Block::EnumItem { number, .. } => BlockKind::EnumItem { number: *number },
            Block::TermItem { .. } => BlockKind::TermItem,
            Block::RawBlock { .. } | Block::Equation { .. } | Block::FuncCall { .. } => {
                BlockKind::Atomic
            }
            Block::Parbreak => BlockKind::Parbreak,
        }
    }

    /// Returns the text content used for word-level diffing.
    pub fn diff_text(&self) -> Option<String> {
        match self {
            Block::Paragraph { source_text } => Some(source_text.clone()),
            Block::Heading { body_text, .. } => Some(body_text.clone()),
            Block::ListItem { body_text } => Some(body_text.clone()),
            Block::EnumItem { body_text, .. } => Some(body_text.clone()),
            Block::TermItem { term, description } => Some(format!("{term}: {description}")),
            _ => None,
        }
    }

    /// Returns true if this block should be treated as atomic (no word-level diff).
    pub fn is_atomic(&self) -> bool {
        matches!(
            self,
            Block::RawBlock { .. }
                | Block::Equation { .. }
                | Block::FuncCall { .. }
                | Block::Parbreak
        )
    }
}

/// The kind of block, used to determine if two blocks can be merged into a Modified result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading { depth: usize },
    ListItem,
    EnumItem { number: Option<usize> },
    TermItem,
    Atomic,
    Parbreak,
}

/// Result of diffing two documents at the block level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffResult {
    Unchanged(Block),
    Added(Block),
    Deleted(Block),
    Modified {
        kind: BlockKind,
        spans: Vec<DiffSpan>,
    },
}

/// A span within a modified block, representing equal, deleted, or inserted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSpan {
    pub tag: SpanTag,
    pub text: String,
}

/// Tag for a diff span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanTag {
    Equal,
    Deleted,
    Inserted,
}
