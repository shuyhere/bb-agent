use std::collections::{BTreeSet, HashMap};

use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(u64);

impl BlockId {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    UserMessage,
    AssistantMessage,
    Thinking,
    ToolUse,
    ToolResult,
    SystemNote,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptBlock {
    pub id: BlockId,
    pub kind: BlockKind,
    pub title: String,
    pub content: String,
    pub collapsed: bool,
    pub expandable: bool,
    pub parent: Option<BlockId>,
    pub children: Vec<BlockId>,
    pub dirty: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewBlock {
    pub kind: BlockKind,
    pub title: String,
    pub content: String,
    pub collapsed: bool,
    pub expandable: bool,
}

impl NewBlock {
    pub fn new(kind: BlockKind, title: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            content: String::new(),
            collapsed: false,
            expandable: false,
        }
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    pub fn with_collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    pub fn with_expandable(mut self, expandable: bool) -> Self {
        self.expandable = expandable;
        self
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TranscriptError {
    #[error("unknown transcript block id {0:?}")]
    UnknownBlock(BlockId),
    #[error("transcript block {id:?} is not a tool result")]
    NotToolResult { id: BlockId },
}

#[derive(Clone, Debug, Default)]
pub struct Transcript {
    next_block_id: u64,
    root_blocks: Vec<BlockId>,
    blocks: HashMap<BlockId, TranscriptBlock>,
}

impl Transcript {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn root_blocks(&self) -> &[BlockId] {
        &self.root_blocks
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn block(&self, id: BlockId) -> Option<&TranscriptBlock> {
        self.blocks.get(&id)
    }

    pub fn append_root_block(&mut self, block: NewBlock) -> BlockId {
        let id = self.next_id();
        let block = TranscriptBlock {
            id,
            kind: block.kind,
            title: block.title,
            content: block.content,
            collapsed: block.collapsed,
            expandable: block.expandable,
            parent: None,
            children: Vec::new(),
            dirty: true,
        };

        self.root_blocks.push(id);
        self.blocks.insert(id, block);
        id
    }

    pub fn append_child_block(
        &mut self,
        parent_id: BlockId,
        block: NewBlock,
    ) -> Result<BlockId, TranscriptError> {
        self.require_block(parent_id)?;

        let id = self.next_id();
        let block = TranscriptBlock {
            id,
            kind: block.kind,
            title: block.title,
            content: block.content,
            collapsed: block.collapsed,
            expandable: block.expandable,
            parent: Some(parent_id),
            children: Vec::new(),
            dirty: true,
        };

        self.blocks.insert(id, block);

        let parent = self
            .blocks
            .get_mut(&parent_id)
            .ok_or(TranscriptError::UnknownBlock(parent_id))?;
        parent.children.push(id);
        parent.expandable = true;
        parent.dirty = true;

        Ok(id)
    }

    pub fn update_title(
        &mut self,
        id: BlockId,
        title: impl Into<String>,
    ) -> Result<(), TranscriptError> {
        let block = self.block_mut(id)?;
        block.title = title.into();
        block.dirty = true;
        Ok(())
    }

    pub fn append_streamed_content(
        &mut self,
        id: BlockId,
        content: impl AsRef<str>,
    ) -> Result<(), TranscriptError> {
        let block = self.block_mut(id)?;
        block.content.push_str(content.as_ref());
        block.dirty = true;
        Ok(())
    }

    pub fn replace_content(
        &mut self,
        id: BlockId,
        content: impl Into<String>,
    ) -> Result<(), TranscriptError> {
        let block = self.block_mut(id)?;
        block.content = content.into();
        block.dirty = true;
        Ok(())
    }

    pub fn set_collapsed(&mut self, id: BlockId, collapsed: bool) -> Result<(), TranscriptError> {
        let block = self.block_mut(id)?;
        block.collapsed = collapsed;
        block.dirty = true;
        Ok(())
    }

    pub fn set_expanded(&mut self, id: BlockId) -> Result<(), TranscriptError> {
        self.set_collapsed(id, false)
    }

    pub fn replace_tool_result_content(
        &mut self,
        id: BlockId,
        content: impl Into<String>,
    ) -> Result<(), TranscriptError> {
        let block = self.block_mut(id)?;
        if block.kind != BlockKind::ToolResult {
            return Err(TranscriptError::NotToolResult { id });
        }

        block.content = content.into();
        block.dirty = true;
        Ok(())
    }

    pub fn mark_block_dirty(&mut self, id: BlockId) -> Result<(), TranscriptError> {
        let block = self.block_mut(id)?;
        block.dirty = true;
        Ok(())
    }

    pub fn take_dirty_blocks(&mut self) -> BTreeSet<BlockId> {
        let mut dirty = BTreeSet::new();
        for block in self.blocks.values_mut() {
            if block.dirty {
                dirty.insert(block.id);
                block.dirty = false;
            }
        }
        dirty
    }

    pub fn has_dirty_blocks(&self) -> bool {
        self.blocks.values().any(|block| block.dirty)
    }

    pub fn all_block_ids(&self) -> BTreeSet<BlockId> {
        self.blocks.keys().copied().collect()
    }

    fn next_id(&mut self) -> BlockId {
        self.next_block_id += 1;
        BlockId(self.next_block_id)
    }

    fn require_block(&self, id: BlockId) -> Result<(), TranscriptError> {
        if self.blocks.contains_key(&id) {
            Ok(())
        } else {
            Err(TranscriptError::UnknownBlock(id))
        }
    }

    fn block_mut(&mut self, id: BlockId) -> Result<&mut TranscriptBlock, TranscriptError> {
        self.blocks
            .get_mut(&id)
            .ok_or(TranscriptError::UnknownBlock(id))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{BlockKind, NewBlock, Transcript};

    #[test]
    fn append_root_and_child_blocks() {
        let mut transcript = Transcript::new();
        let root =
            transcript.append_root_block(NewBlock::new(BlockKind::AssistantMessage, "assistant"));
        let child = transcript
            .append_child_block(root, NewBlock::new(BlockKind::Thinking, "thinking"))
            .expect("child block should be appended");

        assert_eq!(transcript.root_blocks(), &[root]);

        let root_block = transcript.block(root).expect("root block should exist");
        assert_eq!(root_block.parent, None);
        assert_eq!(root_block.children, vec![child]);
        assert!(root_block.expandable);

        let child_block = transcript.block(child).expect("child block should exist");
        assert_eq!(child_block.parent, Some(root));
        assert!(child_block.children.is_empty());
    }

    #[test]
    fn collapse_and_expand_state_changes() {
        let mut transcript = Transcript::new();
        let block = transcript
            .append_root_block(NewBlock::new(BlockKind::ToolUse, "tool").with_expandable(true));

        transcript
            .set_collapsed(block, true)
            .expect("collapse should succeed");
        assert!(
            transcript
                .block(block)
                .expect("block should exist")
                .collapsed
        );

        transcript
            .set_expanded(block)
            .expect("expand should succeed");
        assert!(
            !transcript
                .block(block)
                .expect("block should exist")
                .collapsed
        );
    }

    #[test]
    fn streamed_append_preserves_existing_content() {
        let mut transcript = Transcript::new();
        let block = transcript.append_root_block(
            NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content("Hello"),
        );

        transcript
            .append_streamed_content(block, ", world")
            .expect("first append should succeed");
        transcript
            .append_streamed_content(block, "!")
            .expect("second append should succeed");

        assert_eq!(
            transcript.block(block).expect("block should exist").content,
            "Hello, world!"
        );
    }

    #[test]
    fn parent_child_ordering_is_preserved() {
        let mut transcript = Transcript::new();
        let assistant =
            transcript.append_root_block(NewBlock::new(BlockKind::AssistantMessage, "assistant"));
        let thinking = transcript
            .append_child_block(assistant, NewBlock::new(BlockKind::Thinking, "thinking"))
            .expect("thinking child should be appended");
        let tool_use = transcript
            .append_child_block(assistant, NewBlock::new(BlockKind::ToolUse, "tool use"))
            .expect("tool use child should be appended");
        let tool_result = transcript
            .append_child_block(
                tool_use,
                NewBlock::new(BlockKind::ToolResult, "tool result"),
            )
            .expect("tool result child should be appended");
        let assistant_content = transcript
            .append_child_block(
                assistant,
                NewBlock::new(BlockKind::AssistantMessage, "assistant content"),
            )
            .expect("assistant content child should be appended");

        let assistant_block = transcript
            .block(assistant)
            .expect("assistant block should exist");
        assert_eq!(
            assistant_block.children,
            vec![thinking, tool_use, assistant_content]
        );

        let tool_use_block = transcript
            .block(tool_use)
            .expect("tool use block should exist");
        assert_eq!(tool_use_block.children, vec![tool_result]);
    }

    #[test]
    fn block_lookup_by_id_returns_expected_block() {
        let mut transcript = Transcript::new();
        let block = transcript.append_root_block(
            NewBlock::new(BlockKind::SystemNote, "note").with_content("created"),
        );

        let looked_up = transcript.block(block).expect("block should exist");
        assert_eq!(looked_up.id, block);
        assert_eq!(looked_up.kind, BlockKind::SystemNote);
        assert_eq!(looked_up.title, "note");
        assert_eq!(looked_up.content, "created");
    }

    #[test]
    fn replace_tool_result_content_replaces_existing_value() {
        let mut transcript = Transcript::new();
        let block = transcript.append_root_block(
            NewBlock::new(BlockKind::ToolResult, "tool result").with_content("old"),
        );

        transcript
            .replace_tool_result_content(block, "new")
            .expect("tool result replacement should succeed");

        assert_eq!(
            transcript.block(block).expect("block should exist").content,
            "new"
        );
    }

    #[test]
    fn replace_content_updates_any_block_kind() {
        let mut transcript = Transcript::new();
        let block = transcript.append_root_block(
            NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content("old"),
        );

        transcript
            .replace_content(block, "new")
            .expect("replacement should succeed");

        assert_eq!(
            transcript.block(block).expect("block should exist").content,
            "new"
        );
    }

    #[test]
    fn take_dirty_blocks_returns_only_changed_ids() {
        let mut transcript = Transcript::new();
        let first = transcript.append_root_block(
            NewBlock::new(BlockKind::AssistantMessage, "assistant").with_content("hello"),
        );
        let second = transcript
            .append_root_block(NewBlock::new(BlockKind::SystemNote, "note").with_content("world"));
        let _ = transcript.take_dirty_blocks();

        transcript
            .append_streamed_content(first, "!")
            .expect("streaming append should succeed");
        let dirty = transcript.take_dirty_blocks();

        assert_eq!(dirty, BTreeSet::from([first]));
        assert!(!dirty.contains(&second));
    }
}
