use crate::models::line;

use super::line::*;

pub struct Lines{
	lines: Vec<Line>,
	actual_line: u16
}

impl Lines{
	pub fn new()->Self{
		let mut ob = Self{
			lines: Vec::new(),
			actual_line: 0
		};
		
		ob.lines.push(
			line::Line::new()
		);
		
		ob
	}
	
	pub fn putchar(&mut self,c: char){
		self.lines[self.actual_line as usize].push_ch(c);
	}
	
	pub fn newline(){
		
	}
	
	pub fn is_current_line_active(&self)->bool{
		self.lines[self.actual_line as usize].is_active
	}
	
	pub fn active_current_line(&mut self){
		self.lines[self.actual_line as usize].is_active = true;
	}
}