use std::{
    collections::{HashMap, VecDeque},
    io::Read,
    path::PathBuf,
};

use crate::tree::Node;

const TARGET_CRATE: &str = "ipa";
const STEPS_FILE_PATH: &str = "/../src/protocol/step/";
pub(crate) const STEPS_FILE_NAME: &str = "steps.txt";

#[derive(Clone, Debug)]
pub(crate) struct StepMetaData {
    pub id: u16,
    pub depth: u8,
    pub module: String,
    pub name: String,
    pub path: String,
}

impl StepMetaData {
    pub fn new(id: u16, depth: u8, module: String, name: String, path: String) -> Self {
        Self {
            id,
            depth,
            module,
            name,
            path,
        }
    }
}

/// Generate the state transition map. This is implemented as a tree where each node represents
/// a narrowed step. The root node represents the root step, and each child node represents a
/// narrowed step. The tree is generated by reading the steps file where each line represents a
/// hierarchy of steps delimited by "/".
pub(crate) fn ipa_state_transition_map() -> Node<StepMetaData> {
    let steps = read_steps_file(STEPS_FILE_NAME)
        .into_iter()
        .enumerate()
        .map(|(i, path)| {
            let id = u16::try_from(i + 1).unwrap();
            let path_list = path
                .split("/")
                .map(|s| split_step_module_and_name(s))
                .collect::<Vec<_>>();
            let depth = u8::try_from(path_list.len()).unwrap();
            let (module, name) = path_list.last().unwrap();
            // `path` is used to construct the AsRef implementation.
            // strip the module parts from all steps to reduce the memory footprint.
            let path = path_list
                .iter()
                .map(|(_, name)| name.to_owned())
                .collect::<Vec<_>>()
                .join("/");
            StepMetaData::new(id, depth, module.to_owned(), name.to_owned(), path)
        })
        .collect::<Vec<_>>();

    construct_tree(steps)
}

/// Reads the steps file and returns a vector of strings, where each string represents a line in the file.
pub(crate) fn read_steps_file(file_path: &str) -> Vec<String> {
    // construct the path to the steps file saved in STEPS_FILE_PATH relative to this crate's root.
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR").to_owned() + STEPS_FILE_PATH);
    path.push(file_path);

    // expect that there's always a steps file
    let mut file = std::fs::File::open(path).expect("Could not open the steps file");
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    contents.lines().map(|s| s.to_owned()).collect::<Vec<_>>()
}

/// Constructs a tree structure with nodes that contain the `Step` instances.
/// Tree structure helps us to easily find the parent of the current step.
pub(crate) fn construct_tree(steps: Vec<StepMetaData>) -> Node<StepMetaData> {
    let root = Node::new(StepMetaData::new(
        0,
        0,
        TARGET_CRATE.to_string(),
        "root".to_string(),
        "root".to_string(),
    ));
    let mut last_node = root.clone();

    // This logic is based on the assumption that the steps file is sorted by alphabetical order,
    // so that steps are grouped by their parents. Another way of doing this is to introduce
    // another loop to find the parent node from `steps`, but that would be O(n^2).
    for step in steps {
        let delta = i32::try_from(last_node.depth).unwrap() - i32::try_from(step.depth).unwrap();
        let parent = {
            // The implication of the following statement is that, if `delta` is:
            //   = -1, the new state has transitioned one level down. `last_node` is my parent.
            //   = 0, the new state is on the same level. This step shares the same parent with `last_node`.
            //   > 0, the new state has transitioned `delta` levels up. i.e., `delta = 1` means `last_node`'s grandparent is my parent.
            for _ in 0..=delta {
                last_node = last_node.get_parent().unwrap();
            }
            last_node
        };
        last_node = parent.add_child(step);
    }
    root
}

/// Split a single substep full path into the module path and the step's name.
///
/// # Example
/// input = "ipa::protocol::modulus_conversion::convert_shares::Step::xor1"
/// output = ("ipa::protocol::modulus_conversion::convert_shares::Step", "xor1")
pub(crate) fn split_step_module_and_name(input: &str) -> (String, String) {
    let mod_parts = input.split("::").map(|s| s.to_owned()).collect::<Vec<_>>();
    let (substep_name, path) = mod_parts.split_last().unwrap();
    (path.join("::"), substep_name.to_owned())
}

/// Traverse the tree and group the nodes by their module paths. This is required because sub-steps
/// that are defined in the same enum could be narrowed from different parents.
///
/// # Example
/// Let say we have the following steps:
///
/// - StepA::A1
/// - StepC::C1/StepD::D1/StepA::A2
///
/// If we generate code for each node while traversing, we will end up with the following:
///
/// ```ignore
/// impl StepNarrow<StepA> for Compact { ... }
/// impl StepNarrow<StepC> for Compact { ... }
/// impl StepNarrow<StepD> for Compact { ... }
/// impl StepNarrow<StepA> for Compact { ... }  // error: conflicting implementation of `StepNarrow<StepA>`
/// ```
///
/// Since rust does not allow multiple occurrences of the same impl block, we need to group the nodes.
pub(crate) fn group_by_modules(
    root: &Node<StepMetaData>,
) -> HashMap<String, Vec<Node<StepMetaData>>> {
    let mut result: HashMap<String, Vec<Node<StepMetaData>>> = HashMap::new();
    let mut queue = VecDeque::new();
    queue.extend(root.get_children());

    while let Some(current) = queue.pop_front() {
        if let Some(node) = result.get_mut(&current.module) {
            node.push(current.clone());
        } else {
            result.insert(current.module.clone(), vec![current.clone()]);
        }
        queue.extend(current.get_children());
    }

    result
}