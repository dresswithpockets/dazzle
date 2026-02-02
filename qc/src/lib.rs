use std::{collections::HashMap, fmt::Display};


const MAX_NODE_SIZE: usize = 2048;

#[derive(Debug)]
pub struct Builder {
    size: usize,
    nodes: Vec<Node>,
}

#[derive(Debug)]
pub struct Node {
    size: usize,
    includes: Vec<String>,
}

impl Display for Builder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("$modelname \"precache.mdl\"\n")?;
        for idx in 0..self.nodes.len() {
            write!(f, "$includemodel \"{idx}__qpc.mdl\"\n")?;
        }

        Ok(())
    }
}

impl Builder {
    pub fn new() -> Self {
        Self {
            size: "$modelname \"precache.mdl\"\n".len(),
            nodes: Vec::new(),
        }
    }
    
    pub fn add_model(&mut self, model: String) {
        if self.nodes.is_empty() {
            todo!();
        }
    }

    pub fn to_node_strings() -> HashMap<String, String> {
        todo!();
    }
}

/*
first line is 26 chars including newline
first 10 includes are 10*27=270 chars including newlines
next 62 includes are 62*28=1736 chars including newlines
16 chars of wasted space

total includable size is 147456 chars.
 */

/*
$modelname "precache.mdl"
$includemodel "0__qpc.mdl"
*/