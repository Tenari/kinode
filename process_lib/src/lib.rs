/// Uqbar process standard library for Rust compiled to WASM
/// Must be used in context of bindings generated by uqbar.wit
use serde::{Deserialize, Serialize};
use crate::component::uq_process::api::*;

wit_bindgen::generate!({
    path: "../wit",
    world: "uq-process-lib",
});

/// Override the println! macro to print to the terminal
macro_rules! println {
        () => {
            $print_to_terminal(0, "\n");
        };
        ($($arg:tt)*) => {
            $print_to_terminal(0, &format!($($arg)*));
        };
    }

/// PackageId is like a ProcessId, but for a package. Only contains the name
/// of the package and the name of the publisher.
#[derive(Hash, Eq, PartialEq, Debug, Clone, Serialize, Deserialize)]
pub struct PackageId {
    package_name: String,
    publisher_node: String,
}

impl PackageId {
    pub fn new(package_name: &str, publisher_node: &str) -> Self {
        PackageId {
            package_name: package_name.into(),
            publisher_node: publisher_node.into(),
        }
    }
    pub fn from_str(input: &str) -> Result<Self, ProcessIdParseError> {
        // split string on colons into 2 segments
        let mut segments = input.split(':');
        let package_name = segments
            .next()
            .ok_or(ProcessIdParseError::MissingField)?
            .to_string();
        let publisher_node = segments
            .next()
            .ok_or(ProcessIdParseError::MissingField)?
            .to_string();
        if segments.next().is_some() {
            return Err(ProcessIdParseError::TooManyColons);
        }
        Ok(PackageId {
            package_name,
            publisher_node,
        })
    }
    pub fn to_string(&self) -> String {
        [self.package_name.as_str(), self.publisher_node.as_str()].join(":")
    }
    pub fn package(&self) -> &str {
        &self.package_name
    }
    pub fn publisher_node(&self) -> &str {
        &self.publisher_node
    }
}

/// ProcessId is defined in the wit bindings, but constructors and methods
/// are defined here.
impl ProcessId {
    /// generates a random u64 number if process_name is not declared
    pub fn new(process_name: &str, package_name: &str, publisher_node: &str) -> Self {
        ProcessId {
            process_name: process_name.into(),
            package_name: package_name.into(),
            publisher_node: publisher_node.into(),
        }
    }
    pub fn from_str(input: &str) -> Result<Self, ProcessIdParseError> {
        // split string on colons into 3 segments
        let mut segments = input.split(':');
        let process_name = segments
            .next()
            .ok_or(ProcessIdParseError::MissingField)?
            .to_string();
        let package_name = segments
            .next()
            .ok_or(ProcessIdParseError::MissingField)?
            .to_string();
        let publisher_node = segments
            .next()
            .ok_or(ProcessIdParseError::MissingField)?
            .to_string();
        if segments.next().is_some() {
            return Err(ProcessIdParseError::TooManyColons);
        }
        Ok(ProcessId {
            process_name,
            package_name,
            publisher_node,
        })
    }
    pub fn to_string(&self) -> String {
        [
            self.process_name.as_str(),
            self.package_name.as_str(),
            self.publisher_node.as_str(),
        ]
        .join(":")
    }
    pub fn process(&self) -> &str {
        &self.process_name
    }
    pub fn package(&self) -> &str {
        &self.package_name
    }
    pub fn publisher_node(&self) -> &str {
        &self.publisher_node
    }
}

impl std::fmt::Display for ProcessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.process_name, self.package_name, self.publisher_node
        )
    }
}

impl PartialEq for ProcessId {
    fn eq(&self, other: &Self) -> bool {
        self.process_name == other.process_name
            && self.package_name == other.package_name
            && self.publisher_node == other.publisher_node
    }
}

impl PartialEq<&str> for ProcessId {
    fn eq(&self, other: &&str) -> bool {
        &self.to_string() == other
    }
}

impl PartialEq<ProcessId> for &str {
    fn eq(&self, other: &ProcessId) -> bool {
        self == &other.to_string()
    }
}

#[derive(Debug)]
pub enum ProcessIdParseError {
    TooManyColons,
    MissingField,
}

impl std::fmt::Display for ProcessIdParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ProcessIdParseError::TooManyColons => "Too many colons in ProcessId string",
                ProcessIdParseError::MissingField => "Missing field in ProcessId string",
            }
        )
    }
}

impl std::error::Error for ProcessIdParseError {
    fn description(&self) -> &str {
        match self {
            ProcessIdParseError::TooManyColons => "Too many colons in ProcessId string",
            ProcessIdParseError::MissingField => "Missing field in ProcessId string",
        }
    }
}

/// Address is defined in the wit bindings, but constructors and methods here.
impl Address {
    pub fn from_str(input: &str) -> Result<Self, AddressParseError> {
        // split string on colons into 4 segments,
        // first one with @, next 3 with :
        let mut name_rest = input.split('@');
        let node = name_rest
            .next()
            .ok_or(AddressParseError::MissingField)?
            .to_string();
        let mut segments = name_rest
            .next()
            .ok_or(AddressParseError::MissingNodeId)?
            .split(':');
        let process_name = segments
            .next()
            .ok_or(AddressParseError::MissingField)?
            .to_string();
        let package_name = segments
            .next()
            .ok_or(AddressParseError::MissingField)?
            .to_string();
        let publisher_node = segments
            .next()
            .ok_or(AddressParseError::MissingField)?
            .to_string();
        if segments.next().is_some() {
            return Err(AddressParseError::TooManyColons);
        }
        Ok(Address {
            node,
            process: ProcessId {
                process_name,
                package_name,
                publisher_node,
            },
        })
    }
    pub fn to_string(&self) -> String {
        [self.node.as_str(), &self.process.to_string()].join("@")
    }
}

#[derive(Debug)]
pub enum AddressParseError {
    TooManyColons,
    MissingNodeId,
    MissingField,
}

///
/// Here, we define wrappers over the wit bindings to make them easier to use.
/// This library prescribes the use of IPC and metadata types serialized and
/// deserialized to JSON, which is far from optimal for performance, but useful
/// for applications that want to maximize composability and introspectability.
/// For payloads, we use bincode to serialize and deserialize to bytes.
///

pub fn send_typed_request<T1, T2, T3, T4>(
    target: &Address,
    inherit_payload_and_context: bool,
    ipc: &T1,
    metadata: Option<&T2>,
    context: Option<&T3>,
    payload: Option<&T4>,
    timeout: Option<u64>,
) -> anyhow::Result<()>
where
    T1: serde::Serialize,
    T2: serde::Serialize,
    T3: serde::Serialize,
    T4: serde::Serialize,
{
    let payload = match payload {
        Some(payload) => Some(Payload {
            mime: None,
            bytes: bincode::serialize(payload)?,
        }),
        None => None,
    };
    let context = match context {
        Some(context) => Some(serde_json::to_vec(context)?),
        None => None,
    };
    crate::send_request(
        target,
        &Request {
            inherit: inherit_payload_and_context,
            expects_response: timeout,
            ipc: serde_json::to_vec(ipc)?,
            metadata: match metadata {
                Some(metadata) => Some(serde_json::to_string(metadata)?),
                None => None,
            },
        },
        context.as_ref(),
        payload.as_ref(),
    );
    Ok(())
}

pub fn send_typed_response<T1, T2, T3>(
    inherit_payload: bool,
    ipc: &T1,
    metadata: Option<&T2>,
    payload: Option<&T3>, // will overwrite inherit flag if both are set
) -> anyhow::Result<()>
where
    T1: serde::Serialize,
    T2: serde::Serialize,
    T3: serde::Serialize,
{
    let payload = match payload {
        Some(payload) => Some(Payload {
            mime: None,
            bytes: bincode::serialize(payload)?,
        }),
        None => None,
    };
    crate::send_response(
        &Response {
            inherit: inherit_payload,
            ipc: serde_json::to_vec(ipc)?,
            metadata: match metadata {
                Some(metadata) => Some(serde_json::to_string(metadata)?),
                None => None,
            },
        },
        payload.as_ref(),
    );
    Ok(())
}

pub fn send_and_await_typed_response<T1, T2, T3>(
    target: &Address,
    inherit_payload_and_context: bool,
    ipc: &T1,
    metadata: Option<&T2>,
    payload: Option<&T3>,
    timeout: u64,
) -> anyhow::Result<Result<(Address, Message), SendError>>
where
    T1: serde::Serialize,
    T2: serde::Serialize,
    T3: serde::Serialize,
{
    let payload = match payload {
        Some(payload) => Some(Payload {
            mime: None,
            bytes: bincode::serialize(payload)?,
        }),
        None => None,
    };
    let res = crate::send_and_await_response(
        target,
        &Request {
            inherit: inherit_payload_and_context,
            expects_response: Some(timeout),
            ipc: serde_json::to_vec(ipc)?,
            metadata: match metadata {
                Some(metadata) => Some(serde_json::to_string(metadata)?),
                None => None,
            },
        },
        payload.as_ref(),
    );
    Ok(res)
}

pub fn get_typed_payload<T: serde::de::DeserializeOwned>() -> Option<T> {
    match crate::get_payload() {
        Some(payload) => match bincode::deserialize::<T>(&payload.bytes) {
            Ok(bytes) => Some(bytes),
            Err(_) => None,
        },
        None => None,
    }
}

pub fn get_typed_state<T: serde::de::DeserializeOwned>() -> Option<T> {
    match crate::get_state() {
        Some(bytes) => match bincode::deserialize::<T>(&bytes) {
            Ok(state) => Some(state),
            Err(_) => None,
        },
        None => None,
    }
}

pub fn set_typed_state<T>(state: &T) -> anyhow::Result<()>
where
    T: serde::Serialize,
{
    crate::set_state(&bincode::serialize(state)?);
    Ok(())
}

pub fn grant_messaging(our: &Address, grant_to: &Vec<ProcessId>) -> anyhow::Result<()> {
    let Some(our_messaging_cap) = crate::get_capability(
        our,
        &"\"messaging\"".into()
    ) else {
        // the kernel will always give us this capability, so this should never happen
        return Err(anyhow::anyhow!("failed to get our own messaging capability!"))
    };
    for process in grant_to {
        crate::share_capability(&process, &our_messaging_cap);
    }
    Ok(())
}

pub fn can_message(address: &Address) -> bool {
    crate::get_capability(address, &"\"messaging\"".into()).is_some()
}

///
/// Here, we define types used by various Uqbar runtime components. Use these
/// to interface directly with the kernel, filesystem, virtual filesystem,
/// and other components -- if you have the capability to do so.
///

//  move these to better place!
#[derive(Serialize, Deserialize, Debug)]
pub enum FsAction {
    Write,
    Replace(u128),
    Append(Option<u128>),
    Read(u128),
    ReadChunk(ReadChunkRequest),
    Delete(u128),
    Length(u128),
    //  process state management
    GetState,
    SetState,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ReadChunkRequest {
    pub file_uuid: u128,
    pub start: u64,
    pub length: u64,
}
