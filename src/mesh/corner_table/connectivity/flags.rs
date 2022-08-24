use std::fmt::Display;
use bitflags::bitflags;

use super::traits;

bitflags! {
    pub struct Flags: u8 {
        const IS_DELETED = 0b00000001;
        const IS_VISITED = 0b00000010;
    }
}

impl Default for Flags {
    fn default() -> Self {
        return Self { 
            bits: 0b00000000
        };
    }
}

///
/// Sets visited flag to `false`
/// 
#[inline]
pub fn clear_visited<'a, TEntity, TEntitiesIter>(iter: TEntitiesIter) 
    where TEntity: 'a + traits::Flags, 
    TEntitiesIter: Iterator<Item = &'a TEntity> 
{
    for entity in iter {
        entity.set_visited(false);
    }
}

impl Display for Flags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return write!(f, "{:#010b}", self.bits)
    }
}
