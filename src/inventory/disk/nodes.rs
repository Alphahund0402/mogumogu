//! Compact storage of one analysis: 40 bytes per entry plus one shared name
//! buffer. Both grow in fixed chunks, so growing never copies the index and a
//! large analysis never needs twice its size for a moment.
use std::ops::{Index, IndexMut};

/// Entries per node chunk (2.5 MiB).
const NODE_CHUNK: usize = 1 << 16;
/// Name bytes per chunk; a name never spans two chunks.
const NAME_BITS: u32 = 20;
const NAME_CHUNK: usize = 1 << NAME_BITS;

const DIRECTORY: u8 = 1;
const INCOMPLETE: u8 = 2;

#[derive(Debug)]
pub(super) struct Node {
    pub bytes: u64,
    pub allocated: u64,
    pub parent: u32,
    /// Name chunk in the high bits, offset within it in the low ones.
    name_at: u32,
    /// Folders: files below. Files: entries with the same name and size.
    count: u32,
    pub folders: u32,
    /// Unix seconds; earlier or later times are clamped, they are display data.
    modified: u32,
    name_len: u16,
    flags: u8,
}

const _: () = assert!(size_of::<Node>() == 40);

impl Node {
    pub fn directory(&self) -> bool {
        self.flags & DIRECTORY != 0
    }
    /// False if anything below was not read: its size is a lower bound.
    pub fn complete(&self) -> bool {
        self.flags & INCOMPLETE == 0
    }
    pub fn set_incomplete(&mut self) {
        self.flags |= INCOMPLETE;
    }
    pub fn modified(&self) -> i64 {
        i64::from(self.modified)
    }
    pub fn files(&self) -> u32 {
        if self.directory() { self.count } else { 1 }
    }
    pub fn add_files(&mut self, files: u32) {
        self.count = self.count.saturating_add(files);
    }
    pub fn duplicates(&self) -> u32 {
        if self.directory() { 0 } else { self.count }
    }
    pub fn set_duplicates(&mut self, count: u32) {
        if !self.directory() {
            self.count = count;
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct Nodes {
    chunks: Vec<Vec<Node>>,
    names: Vec<String>,
    len: usize,
}

impl Nodes {
    pub fn len(&self) -> usize {
        self.len
    }

    /// Appends one entry and returns its id. The caller bounds the count and
    /// the name bytes; a name is at most 765 bytes (255 UTF-16 units).
    pub fn push(
        &mut self,
        parent: usize,
        name: &str,
        directory: bool,
        bytes: u64,
        allocated: u64,
        modified: i64,
    ) -> usize {
        if self.names.last().is_none_or(|chunk| chunk.len() + name.len() > NAME_CHUNK) {
            self.names.push(String::new());
        }
        let chunk = self.names.len() - 1;
        let names = &mut self.names[chunk];
        // Doubling from an odd first size would overshoot the chunk by up to half.
        if names.len() + name.len() > names.capacity() {
            let target = (names.capacity() * 2).max(names.len() + name.len()).clamp(4096, NAME_CHUNK);
            names.reserve_exact(target - names.len());
        }
        let name_at = ((chunk as u32) << NAME_BITS) | names.len() as u32;
        names.push_str(name);
        if self.len.is_multiple_of(NODE_CHUNK) {
            self.chunks.push(Vec::new());
        }
        self.chunks[self.len / NODE_CHUNK].push(Node {
            bytes,
            allocated,
            parent: parent as u32,
            name_at,
            count: 0,
            folders: 0,
            modified: modified.clamp(0, i64::from(u32::MAX)) as u32,
            name_len: name.len() as u16,
            flags: if directory { DIRECTORY } else { 0 },
        });
        self.len += 1;
        self.len - 1
    }

    pub fn name(&self, id: usize) -> &str {
        let node = &self[id];
        let start = (node.name_at & (NAME_CHUNK as u32 - 1)) as usize;
        &self.names[(node.name_at >> NAME_BITS) as usize][start..start + usize::from(node.name_len)]
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, &Node)> {
        self.chunks.iter().flatten().enumerate()
    }

    /// Heap held by the index, for measurements.
    pub fn memory_bytes(&self) -> usize {
        self.chunks.iter().map(|c| c.capacity() * size_of::<Node>()).sum::<usize>()
            + self.names.iter().map(String::capacity).sum::<usize>()
    }
}

impl Index<usize> for Nodes {
    type Output = Node;
    fn index(&self, id: usize) -> &Node {
        &self.chunks[id / NODE_CHUNK][id % NODE_CHUNK]
    }
}

impl IndexMut<usize> for Nodes {
    fn index_mut(&mut self, id: usize) -> &mut Node {
        &mut self.chunks[id / NODE_CHUNK][id % NODE_CHUNK]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_nodes_survive_chunk_boundaries() {
        let mut nodes = Nodes::default();
        nodes.push(0, "", true, 0, 0, 0);
        let long = "ä".repeat(382);
        for i in 1..NODE_CHUNK + 10 {
            let name = if i % 1000 == 0 { long.clone() } else { format!("name-{i}") };
            assert_eq!(nodes.push(i / 2, &name, i % 3 == 0, i as u64, 4096, -5), i);
        }
        assert_eq!(nodes.len(), NODE_CHUNK + 10);
        assert_eq!(nodes.name(NODE_CHUNK + 5), format!("name-{}", NODE_CHUNK + 5));
        assert_eq!(nodes.name(4000), long);
        assert_eq!(nodes[NODE_CHUNK + 4].parent as usize, (NODE_CHUNK + 4) / 2);
        assert_eq!(nodes[7].modified(), 0, "times before 1970 are clamped");
        assert!(nodes[3].directory() && !nodes[4].directory());
        assert_eq!(nodes.iter().count(), nodes.len());
        nodes[4].set_duplicates(3);
        nodes[3].set_duplicates(3);
        assert_eq!((nodes[4].duplicates(), nodes[3].duplicates(), nodes[4].files()), (3, 0, 1));
        nodes[3].set_incomplete();
        assert!(!nodes[3].complete() && nodes[4].complete());
        assert!(nodes.names.iter().all(|chunk| chunk.capacity() <= NAME_CHUNK), "chunks never overshoot");
    }
}
