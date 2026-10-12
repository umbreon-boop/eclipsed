//! Zero-copy string interning.

use rustc_hash::FxHashMap;

pub type Symbol = u32;

/// Interns slices borrowed from the source; equal strings share a [`Symbol`].
#[derive(Debug, Default)]
pub struct Interner<'source> {
    map: FxHashMap<&'source str, Symbol>,
    data: Vec<&'source str>,
}

impl<'source> Interner<'source> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern(&mut self, string: &'source str) -> Symbol {
        if let Some(&id) = self.map.get(string) {
            return id;
        }

        let id = self.data.len() as u32;
        self.data.push(string);
        self.map.insert(string, id);
        id
    }

    /// # Panics
    ///
    /// Panics if `id` was not produced by this interner.
    pub fn resolve(&self, id: Symbol) -> &'source str {
        self.data[id as usize]
    }

    /// Like [`Interner::resolve`], but `None` for a foreign symbol.
    pub fn get(&self, id: Symbol) -> Option<&'source str> {
        self.data.get(id as usize).copied()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
