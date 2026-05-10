pub struct Line{
	pub line: String,
	pub is_active: bool
}


impl Line{
	pub fn new()-> Self{
		Self{
			line: String::new(),
			is_active: false
		}
	}
	
	pub fn push_ch(&mut self,c: char){
		self.line.push(c);
	}
	
	pub fn pop_ch(&mut self){
		self.line.pop();
	}
}