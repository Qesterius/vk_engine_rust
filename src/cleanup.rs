use std::boxed::Box;

pub type Deletor = Box<dyn FnOnce()>;
// box - for moving allocation from stack to heap
// dyn - type of function is dynamic and can have different sizes
pub struct DeletionQueue{
    deletors: Vec<Deletor>
}

impl DeletionQueue{
    //Rust cannot free vulkan memory on its own. For that task this struct is ...
    pub fn new() -> Self{
        Self { deletors:  Vec::new() }        
    }    
    pub fn push<F>(&mut self, f:F) 
        where 
            F: FnOnce() + 'static, // lifetime requirement. closure of F cannot contain references to any references that could disappear  
    {
        self.deletors.push(Box::new(f));
    }

    pub fn flush(&mut self) 
    {
        for deletor in self.deletors.drain(..).rev(){
            deletor();
        }
    }
}