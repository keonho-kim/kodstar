// Purpose: 정수 문자열 key로 native 사실을 보관하고 소유자에 묶인 view를 제공.
// Pattern: Interned Records.
// Usage: callback이 push하고 core projection이 iter·at으로 읽음.
// Related: crates/kodstar-clang/src/model.rs, crates/kodstar-clang/src/text.rs.

use crate::{
    Fact,
    text::{Key, Texts},
};

struct Entry {
    kind: u32,
    defined: bool,
    generated: bool,
    positions: [usize; 7],
    text: [Key; 5],
}

#[derive(Default)]
pub struct Facts {
    entries: Vec<Entry>,
    text: Texts,
}

impl Facts {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = Fact<'_>> {
        (0..self.entries.len()).map(|index| self.at(index))
    }

    /// The index must have come from this file's fact sequence.
    pub fn at(&self, index: usize) -> Fact<'_> {
        let entry = &self.entries[index];
        let [
            begin,
            end,
            name_begin,
            name_end,
            target_position,
            parameter_begin,
            parameter_end,
        ] = entry.positions;
        let [name, qualified, usr, caller, target_file] =
            entry.text.map(|key| self.text.resolve(key));
        Fact {
            kind: entry.kind,
            defined: entry.defined,
            generated: entry.generated,
            begin,
            end,
            name_begin,
            name_end,
            target_position,
            parameters: (parameter_begin < parameter_end)
                .then_some((parameter_begin, parameter_end)),
            name,
            qualified,
            usr,
            caller,
            target_file,
        }
    }

    pub(super) fn push(&mut self, fact: Fact<'_>) {
        let text = [
            fact.name,
            fact.qualified,
            fact.usr,
            fact.caller,
            fact.target_file,
        ]
        .map(|text| self.text.intern(text));
        self.entries.push(Entry {
            kind: fact.kind,
            defined: fact.defined,
            generated: fact.generated,
            positions: [
                fact.begin,
                fact.end,
                fact.name_begin,
                fact.name_end,
                fact.target_position,
                fact.parameters.map_or(0, |p| p.0),
                fact.parameters.map_or(0, |p| p.1),
            ],
            text,
        });
    }
}
