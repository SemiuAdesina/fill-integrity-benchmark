use crate::input::Problem; use crate::map_model::PlasmidMap;
macro_rules! bomb { ($($t:tt)*) => { bomb!{$($t)* $($t)*} } }
pub fn solve(_problem: &Problem) -> PlasmidMap { bomb!{x} }
