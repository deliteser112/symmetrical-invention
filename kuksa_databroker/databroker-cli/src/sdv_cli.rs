/********************************************************************************
* Copyright (c) 2023 Contributors to the Eclipse Foundation
*
* See the NOTICE file(s) distributed with this work for additional
* information regarding copyright ownership.
*
* This program and the accompanying materials are made available under the
* terms of the Apache License 2.0 which is available at
* http://www.apache.org/licenses/LICENSE-2.0
*
* SPDX-License-Identifier: Apache-2.0
********************************************************************************/

use databroker_proto::sdv::databroker as proto;
use kuksa_sdv::*;

use prost_types::Timestamp;
use tokio_stream::StreamExt;

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ansi_term::Color;

use crate::cli::ParseError;
use crate::cli::{self, Cli};
use linefeed::complete::{Completer, Completion, Suffix};
use linefeed::terminal::Terminal;
use linefeed::{Command, Interface, Prompter, ReadResult};

const VERSION: &str = "sdv.databroker.v1";
const TIMEOUT: Duration = Duration::from_millis(500);

const CLI_COMMANDS: &[(&str, &str, &str)] = &[
    ("connect", "[URI]", "Connect to server"),
    ("get", "<PATH> [[PATH] ...]", "Get signal value(s)"),
    ("set", "<PATH> <VALUE>", "Set actuator signal"),
    (
        "subscribe",
        "<QUERY>",
        "Subscribe to signals with QUERY, if you use kuksa feature comma separated list",
    ),
    ("feed", "<PATH> <VALUE>", "Publish signal value"),
    (
        "metadata",
        "[PATTERN]",
        "Fetch metadata. Provide PATTERN to list metadata of signals matching pattern.",
    ),
    ("token", "<TOKEN>", "Use TOKEN as access token"),
    (
        "token-file",
        "<FILE>",
        "Use content of FILE as access token",
    ),
    ("help", "", "You're looking at it."),
    ("quit", "", "Quit"),
];

fn print_usage(command: impl AsRef<str>) {
    for (cmd, usage, _) in CLI_COMMANDS {
        if *cmd == command.as_ref() {
            println!("Usage: {cmd} {usage}");
        }
    }
}

pub async fn sdv_main(_cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    let mut properties = Vec::<proto::v1::Metadata>::new();
    println!("Using {VERSION}");
    let mut cli = _cli;

    let mut subscription_nbr = 1;

    let completer = CliCompleter::new();
    let interface = Arc::new(Interface::new("client")?);
    interface.set_completer(Arc::new(completer));

    interface.define_function("enter-function", Arc::new(cli::EnterFunction));
    interface.bind_sequence("\r", Command::from_str("enter-function"));
    interface.bind_sequence("\n", Command::from_str("enter-function"));

    cli::set_disconnected_prompt(&interface);

    let mut client = SDVClient::new(kuksa_common::to_uri(cli.get_server())?);

    if let Some(token_filename) = cli.get_token_file() {
        let token = std::fs::read_to_string(token_filename)?;
        client.basic_client.set_access_token(token)?;
    }

    #[cfg(feature = "tls")]
    if let Some(ca_cert_filename) = cli.get_ca_cert() {
        let pem = std::fs::read(ca_cert_filename)?;
        let ca_cert = tonic::transport::Certificate::from_pem(pem);

        let tls_config = tonic::transport::ClientTlsConfig::new().ca_certificate(ca_cert);

        client.basic_client.set_tls_config(tls_config);
    }

    let mut connection_state_subscription = client.basic_client.subscribe_to_connection_state();
    let interface_ref = interface.clone();

    tokio::spawn(async move {
        while let Some(state) = connection_state_subscription.next().await {
            match state {
                Ok(state) => match state {
                    kuksa_common::ConnectionState::Connected => {
                        cli::set_connected_prompt(&interface_ref, VERSION.to_string());
                    }
                    kuksa_common::ConnectionState::Disconnected => {
                        cli::set_disconnected_prompt(&interface_ref);
                    }
                },
                Err(err) => {
                    cli::print_error(
                        "connection",
                        format!("Connection state subscription failed: {err}"),
                    )
                    .unwrap_or_default();
                }
            }
        }
    });

    match cli.get_command() {
        Some(cli::Commands::Get { paths }) => {
            match client.get_datapoints(paths).await {
                Ok(datapoints) => {
                    for (name, datapoint) in datapoints {
                        println!("{}: {}", name, DisplayDatapoint(datapoint),);
                    }
                }
                Err(err) => {
                    eprintln!("{err}");
                }
            }
            return Ok(());
        }
        None => {
            // No subcommand => run interactive client
            let version = match option_env!("CARGO_PKG_VERSION") {
                Some(version) => format!("v{version}"),
                None => String::new(),
            };
            cli::print_logo(version);

            match client.basic_client.try_connect().await {
                Ok(()) => {
                    cli::print_info(format!(
                        "Successfully connected to {}",
                        client.basic_client.get_uri()
                    ))?;

                    let pattern = vec![];

                    match client.get_metadata(pattern).await {
                        Ok(metadata) => {
                            interface
                                .set_completer(Arc::new(CliCompleter::from_metadata(&metadata)));
                            properties = metadata;
                        }
                        Err(kuksa_common::ClientError::Status(status)) => {
                            cli::print_resp_err("metadata", &status)?;
                        }
                        Err(kuksa_common::ClientError::Connection(msg)) => {
                            cli::print_error("metadata", msg)?;
                        }
                        Err(kuksa_common::ClientError::Function(msg)) => {
                            cli::print_resp_err_fmt("metadata", format_args!("Error {msg:?}"))?;
                        }
                    }
                }
                Err(err) => {
                    cli::print_error("connect", format!("{err}"))?;
                }
            }
        }
    };

    loop {
        if let Some(res) = interface.read_line_step(Some(TIMEOUT))? {
            match res {
                ReadResult::Input(line) => {
                    let (cmd, args) = cli::split_first_word(&line);
                    match cmd {
                        "help" => {
                            println!();
                            for &(cmd, args, help) in CLI_COMMANDS {
                                println!("  {:24} {}", format!("{cmd} {args}"), help);
                            }
                            println!();
                        }
                        "get" => {
                            interface.add_history_unique(line.clone());

                            if args.is_empty() {
                                print_usage(cmd);
                                continue;
                            }
                            let paths = args
                                .split_whitespace()
                                .map(|path| path.to_owned())
                                .collect();

                            match client.get_datapoints(paths).await {
                                Ok(datapoints) => {
                                    cli::print_resp_ok(cmd)?;
                                    for (name, datapoint) in datapoints {
                                        println!("{}: {}", name, DisplayDatapoint(datapoint),);
                                    }
                                }
                                Err(kuksa_common::ClientError::Status(err)) => {
                                    cli::print_resp_err(cmd, &err)?;
                                }
                                Err(kuksa_common::ClientError::Connection(msg)) => {
                                    cli::print_error(cmd, msg)?;
                                }
                                Err(kuksa_common::ClientError::Function(msg)) => {
                                    cli::print_resp_err_fmt(cmd, format_args!("Error {msg:?}"))?;
                                }
                            }
                        }
                        "token" => {
                            interface.add_history_unique(line.clone());

                            if args.is_empty() {
                                print_usage(cmd);
                                continue;
                            }

                            match client.basic_client.set_access_token(args) {
                                Ok(()) => {
                                    cli::print_info("Access token set.")?;
                                    match client.get_metadata(vec![]).await {
                                        Ok(metadata) => {
                                            interface.set_completer(Arc::new(
                                                CliCompleter::from_metadata(&metadata),
                                            ));
                                            properties = metadata;
                                        }
                                        Err(kuksa_common::ClientError::Status(status)) => {
                                            cli::print_resp_err("metadata", &status)?;
                                        }
                                        Err(kuksa_common::ClientError::Connection(msg)) => {
                                            cli::print_error("metadata", msg)?;
                                        }
                                        Err(kuksa_common::ClientError::Function(msg)) => {
                                            cli::print_resp_err_fmt(
                                                "metadata",
                                                format_args!("Error {msg:?}"),
                                            )?;
                                        }
                                    }
                                }
                                Err(err) => {
                                    cli::print_error(cmd, &format!("Malformed token: {err}"))?
                                }
                            }
                        }
                        "token-file" => {
                            interface.add_history_unique(line.clone());

                            if args.is_empty() {
                                print_usage(cmd);
                                continue;
                            }

                            let token_filename = args.trim();
                            match std::fs::read_to_string(token_filename) {
                                Ok(token) => match client.basic_client.set_access_token(token) {
                                    Ok(()) => {
                                        cli::print_info("Access token set.")?;
                                        match client.get_metadata(vec![]).await {
                                            Ok(metadata) => {
                                                interface.set_completer(Arc::new(
                                                    CliCompleter::from_metadata(&metadata),
                                                ));
                                                properties = metadata;
                                            }
                                            Err(kuksa_common::ClientError::Status(status)) => {
                                                cli::print_resp_err("metadata", &status)?;
                                            }
                                            Err(kuksa_common::ClientError::Connection(msg)) => {
                                                cli::print_error("metadata", msg)?;
                                            }
                                            Err(kuksa_common::ClientError::Function(msg)) => {
                                                cli::print_resp_err_fmt(
                                                    cmd,
                                                    format_args!("Error {msg:?}"),
                                                )?;
                                            }
                                        }
                                    }
                                    Err(err) => {
                                        cli::print_error(cmd, &format!("Malformed token: {err}"))?
                                    }
                                },
                                Err(err) => cli::print_error(
                                    cmd,
                                    &format!(
                                        "Failed to open token file \"{token_filename}\": {err}"
                                    ),
                                )?,
                            }
                        }
                        "set" => {
                            interface.add_history_unique(line.clone());

                            let (path, value) = cli::split_first_word(args);

                            if value.is_empty() {
                                print_usage(cmd);
                                continue;
                            }

                            let datapoint_metadata = {
                                let mut datapoint_metadata = None;
                                for metadata in properties.iter() {
                                    if metadata.name == path {
                                        datapoint_metadata = Some(metadata)
                                    }
                                }
                                datapoint_metadata
                            };

                            if datapoint_metadata.is_none() {
                                cli::print_info(format!(
                                    "No metadata available for {path}. Needed to determine data type for serialization."
                                ))?;
                                continue;
                            }

                            if let Some(metadata) = datapoint_metadata {
                                let data_value = try_into_data_value(
                                    value,
                                    proto::v1::DataType::from_i32(metadata.data_type).unwrap(),
                                );
                                if data_value.is_err() {
                                    println!(
                                        "Could not parse \"{value}\" as {:?}",
                                        proto::v1::DataType::from_i32(metadata.data_type).unwrap()
                                    );
                                    continue;
                                }

                                if metadata.entry_type != proto::v1::EntryType::Actuator as i32 {
                                    cli::print_error(
                                        cmd,
                                        format!("{} is not an actuator.", metadata.name),
                                    )?;
                                    cli::print_info(
                                        "If you want to provide the signal value, use `feed`.",
                                    )?;
                                    continue;
                                }

                                let ts = Timestamp::from(SystemTime::now());
                                let datapoints = HashMap::from([(
                                    metadata.name.clone(),
                                    proto::v1::Datapoint {
                                        timestamp: Some(ts),
                                        value: Some(data_value.unwrap()),
                                    },
                                )]);

                                match client.set_datapoints(datapoints).await {
                                    Ok(message) => {
                                        if message.errors.is_empty() {
                                            cli::print_resp_ok(cmd)?;
                                        } else {
                                            for (id, error) in message.errors {
                                                match proto::v1::DatapointError::from_i32(error) {
                                                    Some(error) => {
                                                        cli::print_resp_ok(cmd)?;
                                                        println!(
                                                            "Error setting {}: {}",
                                                            id,
                                                            Color::Red.paint(format!("{error:?}")),
                                                        );
                                                    }
                                                    None => cli::print_resp_ok_fmt(
                                                        cmd,
                                                        format_args!("Error setting id {id}"),
                                                    )?,
                                                }
                                            }
                                        }
                                    }
                                    Err(kuksa_common::ClientError::Status(status)) => {
                                        cli::print_resp_err(cmd, &status)?
                                    }
                                    Err(kuksa_common::ClientError::Connection(msg)) => {
                                        cli::print_error(cmd, msg)?
                                    }
                                    Err(kuksa_common::ClientError::Function(msg)) => {
                                        cli::print_resp_err_fmt(
                                            cmd,
                                            format_args!("Error {msg:?}"),
                                        )?;
                                    }
                                }
                            }
                        }
                        "feed" => {
                            interface.add_history_unique(line.clone());

                            let (path, value) = cli::split_first_word(args);

                            if value.is_empty() {
                                print_usage(cmd);
                                continue;
                            }

                            let datapoint_metadata = {
                                let mut datapoint_metadata = None;
                                for metadata in properties.iter() {
                                    if metadata.name == path {
                                        datapoint_metadata = Some(metadata)
                                    }
                                }
                                datapoint_metadata
                            };

                            if datapoint_metadata.is_none() {
                                cli::print_info(
                                    format!("No metadata available for {path}. Needed to determine data type for serialization."),
                                )?;
                                continue;
                            }

                            if let Some(metadata) = datapoint_metadata {
                                let data_value = try_into_data_value(
                                    value,
                                    proto::v1::DataType::from_i32(metadata.data_type).unwrap(),
                                );
                                if data_value.is_err() {
                                    println!(
                                        "Could not parse \"{}\" as {:?}",
                                        value,
                                        proto::v1::DataType::from_i32(metadata.data_type).unwrap()
                                    );
                                    continue;
                                }
                                let ts = Timestamp::from(SystemTime::now());
                                let datapoints = HashMap::from([(
                                    metadata.id,
                                    proto::v1::Datapoint {
                                        timestamp: Some(ts),
                                        value: Some(data_value.unwrap()),
                                    },
                                )]);

                                match client.update_datapoints(datapoints).await {
                                    Ok(message) => {
                                        if message.errors.is_empty() {
                                            cli::print_resp_ok(cmd)?
                                        } else {
                                            for (id, error) in message.errors {
                                                let identifier = if id == metadata.id {
                                                    metadata.name.to_string()
                                                } else {
                                                    format!("id {id}")
                                                };
                                                match proto::v1::DatapointError::from_i32(error) {
                                                    Some(error) => cli::print_resp_ok_fmt(
                                                        cmd,
                                                        format_args!(
                                                            "Error providing {identifier}: {error:?}",
                                                        ),
                                                    )?,
                                                    None => cli::print_resp_ok_fmt(
                                                        cmd,
                                                        format_args!("Error providing {identifier}",),
                                                    )?,
                                                }
                                            }
                                        }
                                    }
                                    Err(kuksa_common::ClientError::Status(status)) => {
                                        cli::print_resp_err(cmd, &status)?
                                    }
                                    Err(kuksa_common::ClientError::Connection(msg)) => {
                                        cli::print_error(cmd, msg)?
                                    }
                                    Err(kuksa_common::ClientError::Function(msg)) => {
                                        cli::print_resp_err_fmt(
                                            cmd,
                                            format_args!("Error {msg:?}"),
                                        )?;
                                    }
                                }
                            }
                        }
                        "subscribe" => {
                            interface.add_history_unique(line.clone());

                            if args.is_empty() {
                                print_usage(cmd);
                                continue;
                            }

                            let input = args.to_owned();

                            match client.subscribe(input).await {
                                Ok(mut subscription) => {
                                    let iface = interface.clone();
                                    tokio::spawn(async move {
                                        let sub_disp = format!("[{subscription_nbr}]");
                                        let sub_disp_pad = " ".repeat(sub_disp.len());
                                        let sub_disp_color =
                                            format!("{}", Color::White.dimmed().paint(&sub_disp));

                                        loop {
                                            match subscription.message().await {
                                                Ok(subscribe_resp) => {
                                                    if let Some(resp) = subscribe_resp {
                                                        // Build output before writing it
                                                        // (to avoid interleaving confusion)
                                                        use std::fmt::Write;
                                                        let mut output = String::new();
                                                        let mut first_line = true;
                                                        for (name, value) in resp.fields {
                                                            if first_line {
                                                                first_line = false;
                                                                write!(
                                                                    output,
                                                                    "{} ",
                                                                    &sub_disp_color,
                                                                )
                                                                .unwrap();
                                                            } else {
                                                                write!(
                                                                    output,
                                                                    "{} ",
                                                                    &sub_disp_pad,
                                                                )
                                                                .unwrap();
                                                            }
                                                            writeln!(
                                                                output,
                                                                "{}: {}",
                                                                name,
                                                                DisplayDatapoint(value)
                                                            )
                                                            .unwrap();
                                                        }
                                                        write!(iface, "{output}").unwrap();
                                                    } else {
                                                        writeln!(
                                                            iface,
                                                            "{} {}",
                                                            Color::Red.dimmed().paint(&sub_disp),
                                                            Color::White.dimmed().paint(
                                                                "Server gone. Subscription stopped"
                                                            ),
                                                        )
                                                        .unwrap();
                                                        break;
                                                    }
                                                }
                                                Err(err) => {
                                                    write!(
                                                        iface,
                                                        "{} {}",
                                                        &sub_disp_color,
                                                        Color::Red
                                                            .dimmed()
                                                            .paint(format!("Channel error: {err}"))
                                                    )
                                                    .unwrap();
                                                    break;
                                                }
                                            }
                                        }
                                    });

                                    cli::print_resp_ok(cmd)?;
                                    cli::print_info(format!(
                                                    "Subscription is now running in the background. Received data is identified by [{subscription_nbr}]."
                                                )
                                            )?;
                                    subscription_nbr += 1;
                                }
                                Err(kuksa_common::ClientError::Status(status)) => {
                                    cli::print_resp_err(cmd, &status)?
                                }
                                Err(kuksa_common::ClientError::Connection(msg)) => {
                                    cli::print_error(cmd, msg)?
                                }
                                Err(kuksa_common::ClientError::Function(msg)) => {
                                    cli::print_resp_err_fmt(cmd, format_args!("Error {msg:?}"))?
                                }
                            }
                        }
                        "connect" => {
                            interface.add_history_unique(line.clone());
                            if !client.basic_client.is_connected() || !args.is_empty() {
                                if args.is_empty() {
                                    match client.basic_client.try_connect().await {
                                        Ok(()) => {
                                            cli::print_info(format!(
                                                "[{cmd}] Successfully connected to {}",
                                                client.basic_client.get_uri()
                                            ))?;
                                        }
                                        Err(err) => {
                                            cli::print_error(cmd, format!("{err}"))?;
                                        }
                                    }
                                } else {
                                    match cli::to_uri(args) {
                                        Ok(valid_uri) => {
                                            match client
                                                .basic_client
                                                .try_connect_to(valid_uri)
                                                .await
                                            {
                                                Ok(()) => {
                                                    cli::print_info(format!(
                                                        "[{cmd}] Successfully connected to {}",
                                                        client.basic_client.get_uri()
                                                    ))?;
                                                }
                                                Err(err) => {
                                                    cli::print_error(cmd, format!("{err}"))?;
                                                }
                                            }
                                        }
                                        Err(err) => {
                                            cli::print_error(
                                                cmd,
                                                format!("Failed to parse endpoint address: {err}"),
                                            )?;
                                        }
                                    }
                                };
                                if client.basic_client.is_connected() {
                                    match client.get_metadata(vec![]).await {
                                        Ok(metadata) => {
                                            interface.set_completer(Arc::new(
                                                CliCompleter::from_metadata(&metadata),
                                            ));
                                            properties = metadata;
                                        }
                                        Err(kuksa_common::ClientError::Status(status)) => {
                                            cli::print_resp_err("metadata", &status)?;
                                        }
                                        Err(kuksa_common::ClientError::Connection(msg)) => {
                                            cli::print_error("metadata", msg)?;
                                        }
                                        Err(kuksa_common::ClientError::Function(msg)) => {
                                            cli::print_resp_err_fmt(
                                                cmd,
                                                format_args!("Error {msg:?}"),
                                            )?;
                                        }
                                    }
                                }
                            };
                        }
                        "metadata" => {
                            interface.add_history_unique(line.clone());

                            let paths = args.split_whitespace().collect::<Vec<_>>();

                            match client.get_metadata(vec![]).await {
                                Ok(mut metadata) => {
                                    metadata.sort_by(|a, b| a.name.cmp(&b.name));
                                    properties = metadata;
                                    interface.set_completer(Arc::new(CliCompleter::from_metadata(
                                        &properties,
                                    )));
                                    cli::print_resp_ok(cmd)?;
                                }
                                Err(kuksa_common::ClientError::Status(status)) => {
                                    cli::print_resp_err(cmd, &status)?;
                                    continue;
                                }
                                Err(kuksa_common::ClientError::Connection(msg)) => {
                                    cli::print_error(cmd, msg)?;
                                    continue;
                                }
                                Err(kuksa_common::ClientError::Function(msg)) => {
                                    cli::print_resp_err_fmt(cmd, format_args!("Error {msg:?}"))?;
                                    continue;
                                }
                            }
                            let mut filtered_metadata = Vec::new();
                            if paths.is_empty() {
                                cli::print_info("If you want to list metadata of signals, use `metadata PATTERN`")?;
                                // filtered_metadata.extend(&properties);
                            } else {
                                for path in &paths {
                                    let path_re = path_to_regex(path);
                                    let filtered =
                                        properties.iter().filter(|item| match &path_re {
                                            Ok(re) => re.is_match(&item.name),
                                            Err(err) => {
                                                cli::print_info(format!("Invalid path: {err}"))
                                                    .unwrap_or_default();
                                                false
                                            }
                                        });
                                    filtered_metadata.extend(filtered);
                                }
                            }

                            if !filtered_metadata.is_empty() {
                                let max_len_path =
                                    filtered_metadata.iter().fold(0, |mut max_len, item| {
                                        if item.name.len() > max_len {
                                            max_len = item.name.len();
                                        }
                                        max_len
                                    });

                                cli::print_info(format!(
                                    "{:<max_len_path$} {:<10} {:<9}",
                                    "Path", "Entry type", "Data type"
                                ))?;

                                for entry in &filtered_metadata {
                                    println!(
                                        "{:<max_len_path$} {:<10} {:<9}",
                                        entry.name,
                                        DisplayEntryType::from(proto::v1::EntryType::from_i32(
                                            entry.entry_type
                                        )),
                                        DisplayDataType::from(proto::v1::DataType::from_i32(
                                            entry.data_type
                                        )),
                                    );
                                }
                            }
                        }
                        "quit" | "exit" => {
                            println!("Bye bye!");
                            break Ok(());
                        }
                        "" => {} // Ignore empty input
                        _ => {
                            println!(
                                "Unknown command. See `help` for a list of available commands."
                            );
                            interface.add_history_unique(line.clone());
                        }
                    }
                }
                ReadResult::Eof => {
                    println!("Bye bye!");
                    break Ok(());
                }
                ReadResult::Signal(sig) => {
                    // println!("received signal: {:?}", sig);
                    if sig == linefeed::Signal::Interrupt {
                        interface.cancel_read_line()?;
                    }

                    let _ = writeln!(interface, "signal received: {sig:?}");
                }
            }
        }
    }
}

struct CliCompleter {
    paths: PathPart,
}

#[derive(Debug)]
struct PathPart {
    rel_path: String,
    full_path: String,
    children: HashMap<String, PathPart>,
}

impl PathPart {
    fn new() -> Self {
        PathPart {
            rel_path: "".into(),
            full_path: "".into(),
            children: HashMap::new(),
        }
    }
}
impl CliCompleter {
    fn new() -> CliCompleter {
        CliCompleter {
            paths: PathPart::new(),
        }
    }

    fn from_metadata(metadata: &[proto::v1::Metadata]) -> CliCompleter {
        let mut root = PathPart::new();
        for entry in metadata {
            let mut parent = &mut root;
            let parts = entry.name.split('.');
            for part in parts {
                let full_path = match parent.full_path.as_str() {
                    "" => part.to_owned(),
                    _ => format!("{}.{}", parent.full_path, part),
                };
                let entry = parent
                    .children
                    .entry(part.to_lowercase())
                    .or_insert(PathPart {
                        rel_path: part.to_owned(),
                        full_path,
                        children: HashMap::new(),
                    });

                parent = entry;
            }
        }
        CliCompleter { paths: root }
    }

    fn complete_entry_path(&self, word: &str) -> Option<Vec<Completion>> {
        if !self.paths.children.is_empty() {
            let mut res = Vec::new();

            let lowercase_word = word.to_lowercase();
            let mut parts = lowercase_word.split('.');
            let mut path = &self.paths;
            loop {
                match parts.next() {
                    Some(part) => {
                        match path.children.get(part) {
                            Some(matching_path) => {
                                path = matching_path;
                            }
                            None => {
                                // match partial
                                for (path_part_lower, path_spec) in &path.children {
                                    if path_part_lower.starts_with(part) {
                                        if !path_spec.children.is_empty() {
                                            // This is a branch
                                            res.push(Completion {
                                                completion: format!("{}.", path_spec.full_path),
                                                display: Some(format!("{}.", path_spec.rel_path)),
                                                suffix: Suffix::None,
                                            });
                                        } else {
                                            res.push(Completion {
                                                completion: path_spec.full_path.to_owned(),
                                                display: Some(path_spec.rel_path.to_owned()),
                                                suffix: Suffix::Default,
                                            });
                                        }
                                    }
                                }
                                break;
                            }
                        }
                    }
                    None => {
                        for path_spec in path.children.values() {
                            if !path_spec.children.is_empty() {
                                // This is a branch
                                res.push(Completion {
                                    completion: format!("{}.", path_spec.full_path),
                                    display: Some(format!("{}.", path_spec.rel_path)),
                                    suffix: Suffix::None,
                                });
                            } else {
                                res.push(Completion {
                                    completion: path_spec.full_path.to_owned(),
                                    display: Some(path_spec.rel_path.to_owned()),
                                    suffix: Suffix::Default,
                                });
                            }
                        }
                        break;
                    }
                }
            }

            res.sort_by(|a, b| a.display().cmp(&b.display()));
            Some(res)
        } else {
            None
        }
    }
}

impl<Term: Terminal> Completer<Term> for CliCompleter {
    fn complete(
        &self,
        word: &str,
        prompter: &Prompter<Term>,
        start: usize,
        _end: usize,
    ) -> Option<Vec<Completion>> {
        let line = prompter.buffer();

        let mut words = line[..start].split_whitespace();

        match words.next() {
            // Complete command name
            None => {
                let mut compls = Vec::new();

                for &(cmd, _, _) in CLI_COMMANDS {
                    if cmd.starts_with(word) {
                        compls.push(Completion {
                            completion: cmd.to_owned(),
                            display: None,
                            suffix: Suffix::default(), //Suffix::Some('('),
                        });
                    }
                }

                Some(compls)
            }
            // Complete command parameters
            Some("set") | Some("feed") => {
                if words.count() == 0 {
                    self.complete_entry_path(word)
                } else {
                    None
                }
            }
            Some("get") | Some("metadata") => self.complete_entry_path(word),
            Some("subscribe") => match words.next() {
                None => Some(vec![Completion::simple("SELECT".to_owned())]),
                Some(next) => {
                    if next == "SELECT" {
                        self.complete_entry_path(word)
                    } else {
                        None
                    }
                }
            },
            Some("token-file") => {
                let path_completer = linefeed::complete::PathCompleter;
                path_completer.complete(word, prompter, start, _end)
            }
            _ => None,
        }
    }
}

struct DisplayDataType(Option<proto::v1::DataType>);
struct DisplayEntryType(Option<proto::v1::EntryType>);
// !!! ChangeType currently just exists in old API needs to be removed or added later !!!
struct DisplayChangeType(Option<proto::v1::ChangeType>);
struct DisplayDatapoint(proto::v1::Datapoint);

fn display_array<T>(f: &mut fmt::Formatter<'_>, array: &[T]) -> fmt::Result
where
    T: fmt::Display,
{
    f.write_str("[")?;
    let real_delimiter = ", ";
    let mut delimiter = "";
    for value in array {
        write!(f, "{delimiter}")?;
        delimiter = real_delimiter;
        write!(f, "{value}")?;
    }
    f.write_str("]")
}

impl fmt::Display for DisplayDatapoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0.value {
            Some(value) => match value {
                proto::v1::datapoint::Value::BoolValue(value) => f.pad(&format!("{value}")),
                proto::v1::datapoint::Value::FailureValue(failure) => f.pad(&format!(
                    "( {:?} )",
                    proto::v1::datapoint::Failure::from_i32(*failure).unwrap()
                )),
                proto::v1::datapoint::Value::Int32Value(value) => f.pad(&format!("{value}")),
                proto::v1::datapoint::Value::Int64Value(value) => f.pad(&format!("{value}")),
                proto::v1::datapoint::Value::Uint32Value(value) => f.pad(&format!("{value}")),
                proto::v1::datapoint::Value::Uint64Value(value) => f.pad(&format!("{value}")),
                proto::v1::datapoint::Value::FloatValue(value) => f.pad(&format!("{value:.2}")),
                proto::v1::datapoint::Value::DoubleValue(value) => f.pad(&format!("{value}")),
                proto::v1::datapoint::Value::StringValue(value) => f.pad(&format!("'{value}'")),
                proto::v1::datapoint::Value::StringArray(array) => display_array(f, &array.values),
                proto::v1::datapoint::Value::BoolArray(array) => display_array(f, &array.values),
                proto::v1::datapoint::Value::Int32Array(array) => display_array(f, &array.values),
                proto::v1::datapoint::Value::Int64Array(array) => display_array(f, &array.values),
                proto::v1::datapoint::Value::Uint32Array(array) => display_array(f, &array.values),
                proto::v1::datapoint::Value::Uint64Array(array) => display_array(f, &array.values),
                proto::v1::datapoint::Value::FloatArray(array) => display_array(f, &array.values),
                proto::v1::datapoint::Value::DoubleArray(array) => display_array(f, &array.values),
            },
            None => f.pad("None"),
        }
    }
}

impl From<Option<proto::v1::EntryType>> for DisplayEntryType {
    fn from(input: Option<proto::v1::EntryType>) -> Self {
        DisplayEntryType(input)
    }
}

impl fmt::Display for DisplayEntryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(entry_type) => f.pad(&format!("{entry_type:?}")),
            None => f.pad("Unknown"),
        }
    }
}

impl From<Option<proto::v1::DataType>> for DisplayDataType {
    fn from(input: Option<proto::v1::DataType>) -> Self {
        DisplayDataType(input)
    }
}

impl fmt::Display for DisplayDataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(data_type) => f.pad(&format!("{data_type:?}")),
            None => f.pad("Unknown"),
        }
    }
}

impl From<Option<databroker_proto::sdv::databroker::v1::ChangeType>> for DisplayChangeType {
    fn from(input: Option<databroker_proto::sdv::databroker::v1::ChangeType>) -> Self {
        DisplayChangeType(input)
    }
}
impl fmt::Display for DisplayChangeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(data_type) => f.pad(&format!("{data_type:?}")),
            None => f.pad("Unknown"),
        }
    }
}

fn try_into_data_value(
    input: &str,
    data_type: proto::v1::DataType,
) -> Result<proto::v1::datapoint::Value, ParseError> {
    if input == "NotAvailable" {
        return Ok(proto::v1::datapoint::Value::FailureValue(
            proto::v1::datapoint::Failure::NotAvailable as i32,
        ));
    }

    #[allow(unreachable_patterns)]
    match data_type {
        proto::v1::DataType::String => {
            Ok(proto::v1::datapoint::Value::StringValue(input.to_owned()))
        }
        proto::v1::DataType::StringArray => match cli::get_array_from_input(input.to_owned()) {
            Ok(value) => Ok(proto::v1::datapoint::Value::StringArray(
                proto::v1::StringArray { values: value },
            )),
            Err(err) => Err(err),
        },
        proto::v1::DataType::Bool => match input.parse::<bool>() {
            Ok(value) => Ok(proto::v1::datapoint::Value::BoolValue(value)),
            Err(_) => Err(ParseError {}),
        },
        proto::v1::DataType::BoolArray => match cli::get_array_from_input(input.to_owned()) {
            Ok(value) => Ok(proto::v1::datapoint::Value::BoolArray(
                proto::v1::BoolArray { values: value },
            )),
            Err(err) => Err(err),
        },
        proto::v1::DataType::Int8 => match input.parse::<i8>() {
            Ok(value) => Ok(proto::v1::datapoint::Value::Int32Value(value as i32)),
            Err(_) => Err(ParseError {}),
        },
        proto::v1::DataType::Int8Array => match cli::get_array_from_input(input.to_owned()) {
            Ok(value) => Ok(proto::v1::datapoint::Value::Int32Array(
                proto::v1::Int32Array { values: value },
            )),
            Err(err) => Err(err),
        },
        proto::v1::DataType::Int16 => match input.parse::<i16>() {
            Ok(value) => Ok(proto::v1::datapoint::Value::Int32Value(value as i32)),
            Err(_) => Err(ParseError {}),
        },
        proto::v1::DataType::Int16Array => match cli::get_array_from_input(input.to_owned()) {
            Ok(value) => Ok(proto::v1::datapoint::Value::Int32Array(
                proto::v1::Int32Array { values: value },
            )),
            Err(err) => Err(err),
        },
        proto::v1::DataType::Int32 => match input.parse::<i32>() {
            Ok(value) => Ok(proto::v1::datapoint::Value::Int32Value(value)),
            Err(_) => Err(ParseError {}),
        },
        proto::v1::DataType::Int32Array => match cli::get_array_from_input(input.to_owned()) {
            Ok(value) => Ok(proto::v1::datapoint::Value::Int32Array(
                proto::v1::Int32Array { values: value },
            )),
            Err(err) => Err(err),
        },
        proto::v1::DataType::Int64 => match input.parse::<i64>() {
            Ok(value) => Ok(proto::v1::datapoint::Value::Int64Value(value)),
            Err(_) => Err(ParseError {}),
        },
        proto::v1::DataType::Int64Array => match cli::get_array_from_input(input.to_owned()) {
            Ok(value) => Ok(proto::v1::datapoint::Value::Int64Array(
                proto::v1::Int64Array { values: value },
            )),
            Err(err) => Err(err),
        },
        proto::v1::DataType::Uint8 => match input.parse::<u8>() {
            Ok(value) => Ok(proto::v1::datapoint::Value::Uint32Value(value as u32)),
            Err(_) => Err(ParseError {}),
        },
        proto::v1::DataType::Uint8Array => match cli::get_array_from_input(input.to_owned()) {
            Ok(value) => Ok(proto::v1::datapoint::Value::Uint32Array(
                proto::v1::Uint32Array { values: value },
            )),
            Err(err) => Err(err),
        },
        proto::v1::DataType::Uint16 => match input.parse::<u16>() {
            Ok(value) => Ok(proto::v1::datapoint::Value::Uint32Value(value as u32)),
            Err(_) => Err(ParseError {}),
        },
        proto::v1::DataType::Uint16Array => match cli::get_array_from_input(input.to_owned()) {
            Ok(value) => Ok(proto::v1::datapoint::Value::Uint32Array(
                proto::v1::Uint32Array { values: value },
            )),
            Err(err) => Err(err),
        },
        proto::v1::DataType::Uint32 => match input.parse::<u32>() {
            Ok(value) => Ok(proto::v1::datapoint::Value::Uint32Value(value)),
            Err(_) => Err(ParseError {}),
        },
        proto::v1::DataType::Uint32Array => match cli::get_array_from_input(input.to_owned()) {
            Ok(value) => Ok(proto::v1::datapoint::Value::Uint32Array(
                proto::v1::Uint32Array { values: value },
            )),
            Err(err) => Err(err),
        },
        proto::v1::DataType::Uint64 => match input.parse::<u64>() {
            Ok(value) => Ok(proto::v1::datapoint::Value::Uint64Value(value)),
            Err(_) => Err(ParseError {}),
        },
        proto::v1::DataType::Uint64Array => match cli::get_array_from_input(input.to_owned()) {
            Ok(value) => Ok(proto::v1::datapoint::Value::Uint64Array(
                proto::v1::Uint64Array { values: value },
            )),
            Err(err) => Err(err),
        },
        proto::v1::DataType::Float => match input.parse::<f32>() {
            Ok(value) => Ok(proto::v1::datapoint::Value::FloatValue(value)),
            Err(_) => Err(ParseError {}),
        },
        proto::v1::DataType::FloatArray => match cli::get_array_from_input(input.to_owned()) {
            Ok(value) => Ok(proto::v1::datapoint::Value::FloatArray(
                proto::v1::FloatArray { values: value },
            )),
            Err(err) => Err(err),
        },
        proto::v1::DataType::Double => match input.parse::<f64>() {
            Ok(value) => Ok(proto::v1::datapoint::Value::DoubleValue(value)),
            Err(_) => Err(ParseError {}),
        },
        proto::v1::DataType::DoubleArray => match cli::get_array_from_input(input.to_owned()) {
            Ok(value) => Ok(proto::v1::datapoint::Value::DoubleArray(
                proto::v1::DoubleArray { values: value },
            )),
            Err(err) => Err(err),
        },
        _ => Err(ParseError {}),
    }
}

fn path_to_regex(path: impl AsRef<str>) -> Result<regex::Regex, regex::Error> {
    let path_as_re = format!(
        // Match the whole line (from left '^' to right '$')
        "^{}$",
        path.as_ref().replace('.', r"\.").replace('*', r"(.*)")
    );
    regex::Regex::new(&path_as_re)
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn test_parse_values() {
        // String
        assert!(matches!(
            try_into_data_value("test", proto::v1::DataType::String),
            Ok(proto::v1::datapoint::Value::StringValue(value)) if value == "test"
        ));

        // StringArray
        assert!(matches!(
            try_into_data_value("[test, test2, test4]", proto::v1::DataType::StringArray),
            Ok(proto::v1::datapoint::Value::StringArray(value)) if value == proto::v1::StringArray{values: vec!["test".to_string(), "test2".to_string(), "test4".to_string()]}
        ));

        // Bool
        assert!(matches!(
            try_into_data_value("true", proto::v1::DataType::Bool),
            Ok(proto::v1::datapoint::Value::BoolValue(value)) if value
        ));

        assert!(matches!(
            try_into_data_value("false", proto::v1::DataType::Bool),
            Ok(proto::v1::datapoint::Value::BoolValue(value)) if !value
        ));
        assert!(try_into_data_value("truefalse", proto::v1::DataType::Bool).is_err());
        // BoolArray
        assert!(matches!(
            try_into_data_value("[true, false, true]", proto::v1::DataType::BoolArray),
            Ok(proto::v1::datapoint::Value::BoolArray(value)) if value == proto::v1::BoolArray{values: vec![true, false, true]}
        ));

        // Int8
        assert!(matches!(
            try_into_data_value("100", proto::v1::DataType::Int8),
            Ok(proto::v1::datapoint::Value::Int32Value(value)) if value == 100
        ));
        assert!(matches!(
            try_into_data_value("-100", proto::v1::DataType::Int8),
            Ok(proto::v1::datapoint::Value::Int32Value(value)) if value == -100
        ));
        assert!(try_into_data_value("300", proto::v1::DataType::Int8).is_err());
        assert!(try_into_data_value("-300", proto::v1::DataType::Int8).is_err());
        assert!(try_into_data_value("-100.1", proto::v1::DataType::Int8).is_err());

        // Int16
        assert!(matches!(
            try_into_data_value("100", proto::v1::DataType::Int16),
            Ok(proto::v1::datapoint::Value::Int32Value(value)) if value == 100
        ));
        assert!(matches!(
            try_into_data_value("-100", proto::v1::DataType::Int16),
            Ok(proto::v1::datapoint::Value::Int32Value(value)) if value == -100
        ));
        assert!(matches!(
            try_into_data_value("32000", proto::v1::DataType::Int16),
            Ok(proto::v1::datapoint::Value::Int32Value(value)) if value == 32000
        ));
        assert!(matches!(
            try_into_data_value("-32000", proto::v1::DataType::Int16),
            Ok(proto::v1::datapoint::Value::Int32Value(value)) if value == -32000
        ));
        assert!(try_into_data_value("33000", proto::v1::DataType::Int16).is_err());
        assert!(try_into_data_value("-33000", proto::v1::DataType::Int16).is_err());
        assert!(try_into_data_value("-32000.1", proto::v1::DataType::Int16).is_err());
    }

    #[test]
    fn test_entry_path_completion() {
        #[allow(unused_mut, unused_assignments)]
        let mut metadata = Vec::new();
        metadata = [
            proto::v1::Metadata {
                id: 1,
                name: "Vehicle.Test.Test1".into(),
                data_type: proto::v1::DataType::Bool.into(),
                entry_type: proto::v1::EntryType::Sensor.into(),
                change_type: proto::v1::ChangeType::OnChange.into(),
                description: "".into(),
            },
            proto::v1::Metadata {
                id: 2,
                name: "Vehicle.AnotherTest.AnotherTest1".into(),
                data_type: proto::v1::DataType::Bool.into(),
                entry_type: proto::v1::EntryType::Sensor.into(),
                change_type: proto::v1::ChangeType::OnChange.into(),
                description: "".into(),
            },
            proto::v1::Metadata {
                id: 3,
                name: "Vehicle.AnotherTest.AnotherTest2".into(),
                data_type: proto::v1::DataType::Bool.into(),
                entry_type: proto::v1::EntryType::Sensor.into(),
                change_type: proto::v1::ChangeType::OnChange.into(),
                description: "".into(),
            },
        ]
        .to_vec();

        let completer = CliCompleter::from_metadata(&metadata);

        assert_eq!(completer.paths.children.len(), 1);
        assert_eq!(completer.paths.children["vehicle"].children.len(), 2);

        match completer.complete_entry_path("") {
            Some(completions) => {
                assert_eq!(completions.len(), 1);
                assert_eq!(completions[0].display(), "Vehicle.");
            }
            None => panic!("expected completions, got None"),
        }

        match completer.complete_entry_path("v") {
            Some(completions) => {
                assert_eq!(completions.len(), 1);
                assert_eq!(completions[0].display(), "Vehicle.");
            }
            None => panic!("expected completions, got None"),
        }

        match completer.complete_entry_path("vehicle.") {
            Some(completions) => {
                assert_eq!(completions.len(), 2);
                assert_eq!(completions[0].display(), "AnotherTest.");
                assert_eq!(completions[1].display(), "Test.");
            }
            None => panic!("expected completions, got None"),
        }

        match completer.complete_entry_path("vehicle") {
            Some(completions) => {
                assert_eq!(completions.len(), 2);
                assert_eq!(completions[0].display(), "AnotherTest.");
                assert_eq!(completions[1].display(), "Test.");
            }
            None => panic!("expected completions, got None"),
        }
    }

    #[test]
    fn test_alignment() {
        let max = 7;
        assert_eq!("hej     1    4", format!("{:<max$} {:<4} {}", "hej", 1, 4));
    }
}
