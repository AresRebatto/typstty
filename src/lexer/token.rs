use std::ops::Range;

use super::token_kind::TokenKind;

pub struct Token{
	kind: TokenKind,
	range: Range<usize>
}