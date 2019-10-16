use std::collections::VecDeque;

pub type Task = Box<dyn FnMut()>;

pub struct ThreadPool {
    tasks: VecDeque<Task>
}