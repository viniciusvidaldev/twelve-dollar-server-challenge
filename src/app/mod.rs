pub enum Handled {
    NeedMore,
    Responded { consumed: usize },
    Close,
}

pub struct App;

impl App {
    pub fn new() -> Self {
        Self
    }

    pub fn handle(&mut self, input: &[u8], out: &mut Vec<u8>) -> Handled {
        todo!()
    }
}
