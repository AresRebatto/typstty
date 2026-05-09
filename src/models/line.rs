pub struct Line{
	line: String,
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
}