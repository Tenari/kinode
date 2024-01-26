use kinode_process_lib::eth::{EthAddress, EthSubEvent, SubscribeLogsRequest};
use kinode_process_lib::http::{bind_http_path, serve_ui, HttpServerRequest};
use kinode_process_lib::kernel_types as kt;
use kinode_process_lib::*;
use kinode_process_lib::{call_init, println};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

mod api;
mod http_api;
use api::*;
mod types;
use types::*;
mod ft_worker_lib;
use ft_worker_lib::{
    spawn_receive_transfer, spawn_transfer, FTWorkerCommand, FTWorkerResult, FileTransferContext,
};

/// App Store:
/// acts as both a local package manager and a protocol to share packages across the network.
/// packages are apps; apps are packages. we use an onchain app listing contract to determine
/// what apps are available to download and what node(s) to download them from.
///
/// once we know that list, we can request a package from a node and download it locally.
/// (we can also manually download an "untracked" package if we know its name and distributor node)
/// packages that are downloaded can then be installed!
///
/// installed packages can be managed:
/// - given permissions (necessary to complete install)
/// - uninstalled + deleted
/// - set to automatically update if a new version is available

const CONTRACT_ADDRESS: &str = "0x6fB8277036ac67AbCBB6d630E33c5f0917b919a3";

// internal types

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all incoming requests
pub enum Req {
    LocalRequest(LocalRequest),
    RemoteRequest(RemoteRequest),
    FTWorkerCommand(FTWorkerCommand),
    FTWorkerResult(FTWorkerResult),
    Eth(EthSubEvent),
    Http(HttpServerRequest),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)] // untagged as a meta-type for all incoming responses
pub enum Resp {
    LocalResponse(LocalResponse),
    RemoteResponse(RemoteResponse),
    FTWorkerResult(FTWorkerResult),
}

call_init!(init);
fn init(our: Address) {
    println!("{}: started", our.package());

    let _ = bind_http_path("/apps", true, false);
    let _ = bind_http_path("/apps/:id", true, false);
    let _ = bind_http_path("/apps/latest", true, false);
    let _ = bind_http_path("/apps/search/:query", true, false);
    let _ = bind_http_path("/apps/publish", true, false);
    let _ = serve_ui(&our, "ui");

    // load in our saved state or initalize a new one if none exists
    let mut state =
        get_typed_state(|bytes| Ok(bincode::deserialize(bytes)?)).unwrap_or(State::new());

    // first, await a message from the kernel which will contain the
    // contract address for the KNS version we want to track.
    let contract_address: Option<String> = Some(CONTRACT_ADDRESS.to_string());
    // loop {
    //     let Ok(Message::Request { source, body, .. }) = await_message() else {
    //         continue;
    //     };
    //     if source.process != "kernel:distro:sys" {
    //         continue;
    //     }
    //     contract_address = Some(std::str::from_utf8(&body).unwrap().to_string());
    //     break;
    // }

    // if contract address changed from a previous run, reset state
    if state.contract_address != contract_address {
        state = State::new();
    }

    println!(
        "app store: indexing on contract address {}",
        contract_address.as_ref().unwrap()
    );

    let mut requested_packages: HashMap<PackageId, RequestedPackage> = HashMap::new();

    // subscribe to events on the app store contract
    SubscribeLogsRequest::new(1) // subscription id 1
        .address(EthAddress::from_str(contract_address.unwrap().as_str()).unwrap())
        .from_block(state.last_saved_block - 1)
        .events(vec![
            "AppRegistered(uint256,string,string,uint256)",
            "AppUnlisted(uint256)",
            "AppMetadataUpdated(uint256,uint256)",
            "Transfer(address,address,uint256)",
        ])
        .send()
        .unwrap();

    loop {
        match await_message() {
            Err(send_error) => {
                // TODO handle these based on what they are triggered by
                println!("app store: got network error: {send_error}");
            }
            Ok(message) => {
                if let Err(e) = handle_message(&our, &mut state, &mut requested_packages, &message)
                {
                    println!("app store: error handling message: {:?}", e)
                }
            }
        }
    }
}

/// message router: parse into our Req and Resp types, then pass to
/// function defined for each kind of message. check whether the source
/// of the message is allowed to send that kind of message to us.
/// finally, fire a response if expected from a request.
fn handle_message(
    our: &Address,
    mut state: &mut State,
    mut requested_packages: &mut HashMap<PackageId, RequestedPackage>,
    message: &Message,
) -> anyhow::Result<()> {
    match message {
        Message::Request {
            source,
            expects_response,
            body,
            ..
        } => match serde_json::from_slice::<Req>(&body)? {
            Req::LocalRequest(local_request) => {
                if our.node != source.node {
                    return Err(anyhow::anyhow!("local request from non-local node"));
                }
                let resp =
                    handle_local_request(&our, &local_request, &mut state, &mut requested_packages);
                if expects_response.is_some() {
                    Response::new().body(serde_json::to_vec(&resp)?).send()?;
                }
            }
            Req::RemoteRequest(remote_request) => {
                let resp = handle_remote_request(&our, &source, &remote_request, &mut state);
                if expects_response.is_some() {
                    Response::new().body(serde_json::to_vec(&resp)?).send()?;
                }
            }
            Req::FTWorkerResult(FTWorkerResult::ReceiveSuccess(name)) => {
                handle_receive_download(&our, &mut state, &mut requested_packages, &name)?;
            }
            Req::FTWorkerCommand(_) => {
                spawn_receive_transfer(&our, &body)?;
            }
            Req::FTWorkerResult(r) => {
                println!("app store: got weird ft_worker result: {r:?}");
            }
            Req::Eth(e) => {
                if source.node() != our.node() || source.process() != "eth:distro:sys" {
                    return Err(anyhow::anyhow!("eth sub event from non-local node"));
                }
                handle_eth_sub_event(&our, &mut state, e)?;
            }
            Req::Http(incoming) => {
                if source.node() != our.node() || source.process() != "http_server:distro:sys" {
                    return Err(anyhow::anyhow!("http_server from non-local node"));
                }
                if let HttpServerRequest::Http(req) = incoming {
                    http_api::handle_http_request(&our, &mut state, &req)?;
                }
            }
        },
        Message::Response { body, context, .. } => {
            // the only kind of response we care to handle here!
            let Some(context) = context else {
                return Err(anyhow::anyhow!("app store: missing context"));
            };
            handle_ft_worker_result(body, context)?;
        }
    }
    Ok(())
}

/// so far just fielding requests to download and grab metadata.
fn handle_remote_request(
    our: &Address,
    source: &Address,
    request: &RemoteRequest,
    state: &mut State,
) -> Resp {
    match request {
        RemoteRequest::Download {
            package_id,
            desired_version_hash,
        } => {
            let Some(package_state) = state.get_downloaded_package(package_id) else {
                return Resp::RemoteResponse(RemoteResponse::DownloadDenied);
            };
            if !package_state.mirroring {
                return Resp::RemoteResponse(RemoteResponse::DownloadDenied);
            }
            if let Some(hash) = desired_version_hash {
                if &package_state.our_version != hash {
                    return Resp::RemoteResponse(RemoteResponse::DownloadDenied);
                }
            }
            // get the .zip from VFS and attach as blob to response
            let file_path = format!("/{}/pkg/{}.zip", package_id, package_id);
            let Ok(Ok(_)) = Request::to(("our", "vfs", "distro", "sys"))
                .body(
                    serde_json::to_vec(&vfs::VfsRequest {
                        path: file_path,
                        action: vfs::VfsAction::Read,
                    })
                    .unwrap(),
                )
                .send_and_await_response(5)
            else {
                return Resp::RemoteResponse(RemoteResponse::DownloadDenied);
            };
            // transfer will *inherit* the blob bytes we receive from VFS
            let file_name = format!("/{}.zip", package_id);
            match spawn_transfer(&our, &file_name, None, 60, &source) {
                Ok(()) => Resp::RemoteResponse(RemoteResponse::DownloadApproved),
                Err(_e) => Resp::RemoteResponse(RemoteResponse::DownloadDenied),
            }
        }
    }
}

/// only `our.node` can call this
fn handle_local_request(
    our: &Address,
    request: &LocalRequest,
    state: &mut State,
    requested_packages: &mut HashMap<PackageId, RequestedPackage>,
) -> LocalResponse {
    match request {
        LocalRequest::NewPackage { package, mirror } => {
            let Some(blob) = get_blob() else {
                return LocalResponse::NewPackageResponse(NewPackageResponse::Failure);
            };
            // set the version hash for this new local package
            let our_version = generate_version_hash(&blob.bytes);
            let package_state = PackageState {
                mirrored_from: Some(our.node.clone()),
                our_version,
                mirroring: *mirror,
                auto_update: false, // can't auto-update a local package
                metadata: None,     // TODO
            };
            match state.add_downloaded_package(blob.bytes, package, package_state) {
                Ok(()) => LocalResponse::NewPackageResponse(NewPackageResponse::Success),
                Err(_) => LocalResponse::NewPackageResponse(NewPackageResponse::Failure),
            }
        }
        LocalRequest::Download {
            package: package_id,
            install_from,
            mirror,
            auto_update,
            desired_version_hash,
        } => LocalResponse::DownloadResponse(
            match Request::to((install_from.as_str(), our.process.clone()))
                .inherit(true)
                .body(
                    serde_json::to_vec(&RemoteRequest::Download {
                        package_id: package_id.clone(),
                        desired_version_hash: desired_version_hash.clone(),
                    })
                    .unwrap(),
                )
                .send_and_await_response(5)
            {
                Ok(Ok(Message::Response { body, .. })) => {
                    match serde_json::from_slice::<Resp>(&body) {
                        Ok(Resp::RemoteResponse(RemoteResponse::DownloadApproved)) => {
                            requested_packages.insert(
                                package_id.clone(),
                                RequestedPackage {
                                    mirror: *mirror,
                                    auto_update: *auto_update,
                                    desired_version_hash: desired_version_hash.clone(),
                                },
                            );
                            DownloadResponse::Started
                        }
                        _ => DownloadResponse::Failure,
                    }
                }
                _ => DownloadResponse::Failure,
            },
        ),
        LocalRequest::Install(package) => match handle_install(our, package) {
            Ok(()) => LocalResponse::InstallResponse(InstallResponse::Success),
            Err(_) => LocalResponse::InstallResponse(InstallResponse::Failure),
        },
        LocalRequest::Uninstall(package) => match state.uninstall(package) {
            Ok(()) => LocalResponse::UninstallResponse(UninstallResponse::Success),
            Err(_) => LocalResponse::UninstallResponse(UninstallResponse::Failure),
        },
        _ => todo!(),
    }
}

fn handle_receive_download(
    our: &Address,
    state: &mut State,
    requested_packages: &mut HashMap<PackageId, RequestedPackage>,
    name: &str,
) -> anyhow::Result<()> {
    // remove leading / and .zip from file name to get package ID
    let name = name[1..].trim_end_matches(".zip");
    let Ok(package_id) = name.parse::<PackageId>() else {
        return Err(anyhow::anyhow!(
            "app store: bad package filename fron download: {name}"
        ));
    };
    println!("app store: successfully received {}", package_id);
    // only install the app if we actually requested it
    let Some(requested_package) = requested_packages.remove(&package_id) else {
        return Err(anyhow::anyhow!(
            "app store: received unrequested package--rejecting!"
        ));
    };
    let Some(blob) = get_blob() else {
        return Err(anyhow::anyhow!(
            "app store: received download but found no blob"
        ));
    };
    // check the version hash for this download against requested!!
    // for now we can reject if it's not latest.
    let download_hash = generate_version_hash(&blob.bytes);
    match requested_package.desired_version_hash {
        Some(hash) => {
            if download_hash != hash {
                return Err(anyhow::anyhow!(
                    "app store: downloaded package is not latest version--rejecting download!"
                ));
            }
        }
        None => {
            // check against latest from listing
            let Some(package_listing) = state.get_listing(&package_id) else {
                return Err(anyhow::anyhow!(
                    "app store: downloaded package cannot be found in manager--rejecting download!"
                ));
            };
            if let Some(metadata) = &package_listing.metadata {
                if let Some(latest_hash) = metadata.versions.first() {
                    if &download_hash != latest_hash {
                        return Err(anyhow::anyhow!(
                            "app store: downloaded package is not latest version--rejecting download!"
                        ));
                    }
                } else {
                    return Err(anyhow::anyhow!(
                        "app store: downloaded package has no versions in manager--rejecting download!"
                    ));
                }
            } else {
                println!("app store: warning: downloaded package has no listing metadata to check validity against!")
            }
        }
    }

    let package_state = PackageState {
        mirrored_from: Some(our.node.clone()),
        our_version: download_hash,
        mirroring: requested_package.mirror,
        auto_update: requested_package.auto_update,
        metadata: None, // TODO
    };
    state.add_downloaded_package(blob.bytes, &package_id, package_state)
}

fn handle_ft_worker_result(body: &[u8], context: &[u8]) -> anyhow::Result<()> {
    if let Ok(Resp::FTWorkerResult(ft_worker_result)) = serde_json::from_slice::<Resp>(body) {
        let context = serde_json::from_slice::<FileTransferContext>(context)?;
        if let FTWorkerResult::SendSuccess = ft_worker_result {
            println!(
                "app store: successfully shared {} in {:.4}s",
                context.file_name,
                std::time::SystemTime::now()
                    .duration_since(context.start_time)
                    .unwrap()
                    .as_secs_f64(),
            );
        } else {
            return Err(anyhow::anyhow!("app store: failed to share app"));
        }
    }
    Ok(())
}

fn handle_eth_sub_event(
    our: &Address,
    state: &mut State,
    event: EthSubEvent,
) -> anyhow::Result<()> {
    let EthSubEvent::Log(log) = event else {
        return Err(anyhow::anyhow!("app store: got non-log event"));
    };
    state.ingest_listings_contract_event(log)
}

/// the steps to take an existing package on disk and install/start it
fn handle_install(our: &Address, package: &PackageId) -> anyhow::Result<()> {
    let drive_path = format!("/{}/pkg", package);
    Request::to(("our", "vfs", "distro", "sys"))
        .body(serde_json::to_vec(&vfs::VfsRequest {
            path: format!("{}/manifest.json", drive_path),
            action: vfs::VfsAction::Read,
        })?)
        .send_and_await_response(5)??;
    let Some(blob) = get_blob() else {
        return Err(anyhow::anyhow!("no blob"));
    };
    let manifest = String::from_utf8(blob.bytes)?;
    let manifest = serde_json::from_str::<Vec<kt::PackageManifestEntry>>(&manifest)?;
    // always grant read/write to their drive, which we created for them
    let Some(read_cap) = get_capability(
        &Address::new(&our.node, ("vfs", "distro", "sys")),
        &serde_json::to_string(&serde_json::json!({
            "kind": "read",
            "drive": drive_path,
        }))?,
    ) else {
        return Err(anyhow::anyhow!("app store: no read cap"));
    };
    let Some(write_cap) = get_capability(
        &Address::new(&our.node, ("vfs", "distro", "sys")),
        &serde_json::to_string(&serde_json::json!({
            "kind": "write",
            "drive": drive_path,
        }))?,
    ) else {
        return Err(anyhow::anyhow!("app store: no write cap"));
    };
    let Some(networking_cap) = get_capability(
        &Address::new(&our.node, ("kernel", "distro", "sys")),
        &"\"network\"".to_string(),
    ) else {
        return Err(anyhow::anyhow!("app store: no net cap"));
    };
    // first, for each process in manifest, initialize it
    // then, once all have been initialized, grant them requested caps
    // and finally start them.
    for entry in &manifest {
        let wasm_path = if entry.process_wasm_path.starts_with("/") {
            entry.process_wasm_path.clone()
        } else {
            format!("/{}", entry.process_wasm_path)
        };
        let wasm_path = format!("{}{}", drive_path, wasm_path);
        // build initial caps
        let mut initial_capabilities: HashSet<kt::Capability> = HashSet::new();
        if entry.request_networking {
            initial_capabilities.insert(kt::de_wit_capability(networking_cap.clone()));
        }
        initial_capabilities.insert(kt::de_wit_capability(read_cap.clone()));
        initial_capabilities.insert(kt::de_wit_capability(write_cap.clone()));
        let process_id = format!("{}:{}", entry.process_name, package);
        let Ok(parsed_new_process_id) = process_id.parse::<ProcessId>() else {
            return Err(anyhow::anyhow!("app store: invalid process id!"));
        };
        // kill process if it already exists
        Request::to(("our", "kernel", "distro", "sys"))
            .body(serde_json::to_vec(&kt::KernelCommand::KillProcess(
                parsed_new_process_id.clone(),
            ))?)
            .send()?;

        let _bytes_response = Request::to(("our", "vfs", "distro", "sys"))
            .body(serde_json::to_vec(&vfs::VfsRequest {
                path: wasm_path.clone(),
                action: vfs::VfsAction::Read,
            })?)
            .send_and_await_response(5)??;
        for value in &entry.request_capabilities {
            let mut capability = None;
            match value {
                serde_json::Value::String(process_name) => {
                    if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
                        capability = get_capability(
                            &Address {
                                node: our.node.clone(),
                                process: parsed_process_id.clone(),
                            },
                            "\"messaging\"".into(),
                        );
                    }
                }
                serde_json::Value::Object(map) => {
                    if let Some(process_name) = map.get("process") {
                        if let Ok(parsed_process_id) = process_name
                            .as_str()
                            .unwrap_or_default()
                            .parse::<ProcessId>()
                        {
                            if let Some(params) = map.get("params") {
                                capability = get_capability(
                                    &Address {
                                        node: our.node.clone(),
                                        process: parsed_process_id.clone(),
                                    },
                                    &params.to_string(),
                                );
                            }
                        }
                    }
                }
                _ => {
                    continue;
                }
            }
            if let Some(cap) = capability {
                initial_capabilities.insert(kt::de_wit_capability(cap));
            } else {
                println!(
                    "app-store: no cap: {} for {} to request!",
                    value.to_string(),
                    package
                );
            }
        }
        Request::to(("our", "kernel", "distro", "sys"))
            .body(serde_json::to_vec(&kt::KernelCommand::InitializeProcess {
                id: parsed_new_process_id.clone(),
                wasm_bytes_handle: wasm_path,
                wit_version: None,
                on_exit: entry.on_exit.clone(),
                initial_capabilities,
                public: entry.public,
            })?)
            .inherit(true)
            .send_and_await_response(5)??;
    }
    // THEN, *after* all processes have been initialized, grant caps in manifest
    // TODO for both grants and requests: make the vector of caps
    // and then do one GrantCapabilities message at the end. much faster.
    for entry in &manifest {
        let process_id = format!("{}:{}", entry.process_name, package);
        let Ok(parsed_new_process_id) = process_id.parse::<ProcessId>() else {
            return Err(anyhow::anyhow!("app store: invalid process id!"));
        };
        for value in &entry.grant_capabilities {
            match value {
                serde_json::Value::String(process_name) => {
                    if let Ok(parsed_process_id) = process_name.parse::<ProcessId>() {
                        Request::to(("our", "kernel", "distro", "sys"))
                            .body(
                                serde_json::to_vec(&kt::KernelCommand::GrantCapabilities {
                                    target: parsed_process_id,
                                    capabilities: vec![kt::Capability {
                                        issuer: Address {
                                            node: our.node.clone(),
                                            process: parsed_new_process_id.clone(),
                                        },
                                        params: "\"messaging\"".into(),
                                    }],
                                })
                                .unwrap(),
                            )
                            .send()?;
                    }
                }
                serde_json::Value::Object(map) => {
                    if let Some(process_name) = map.get("process") {
                        if let Ok(parsed_process_id) = process_name
                            .as_str()
                            .unwrap_or_default()
                            .parse::<ProcessId>()
                        {
                            if let Some(params) = map.get("params") {
                                Request::to(("our", "kernel", "distro", "sys"))
                                    .body(serde_json::to_vec(
                                        &kt::KernelCommand::GrantCapabilities {
                                            target: parsed_process_id,
                                            capabilities: vec![kt::Capability {
                                                issuer: Address {
                                                    node: our.node.clone(),
                                                    process: parsed_new_process_id.clone(),
                                                },
                                                params: params.to_string(),
                                            }],
                                        },
                                    )?)
                                    .send()?;
                            }
                        }
                    }
                }
                _ => {
                    continue;
                }
            }
        }
        Request::to(("our", "kernel", "distro", "sys"))
            .body(serde_json::to_vec(&kt::KernelCommand::RunProcess(
                parsed_new_process_id,
            ))?)
            .send_and_await_response(5)??;
    }
    Ok(())
}
