pub struct Line{
	pub line: String
}


impl Line{
	pub fn new()-> Self{
		Self{
			line: String::new(),
		}
	}
	
	pub fn push_ch(&mut self,c: char){
		self.line.push(c);
	}
	
	pub fn pop_ch(&mut self){
		self.line.pop();
	}
}