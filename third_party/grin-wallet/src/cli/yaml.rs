//! Convert the historical grin-wallet YAML command definition into a clap 4
//! `Command`. clap 4 removed its YAML frontend; this small adapter keeps the
//! single source of truth (`grin-wallet.yml`) and avoids yaml-rust.

use clap::{Arg, ArgAction, Command};
use serde_yaml::{Mapping, Value};

fn leak(value: String) -> &'static str {
	Box::leak(value.into_boxed_str())
}

pub fn yaml_command(yaml: &str) -> Command {
	let root: Value = serde_yaml::from_str(yaml).expect("grin-wallet.yml: invalid YAML");
	let map = root.as_mapping().expect("grin-wallet.yml: root mapping");
	let name = string_field(map, "name").unwrap_or_else(|| "grin-wallet".to_string());
	let mut command = Command::new(leak(name));
	if let Some(about) = string_field(map, "about") {
		command = command.about(leak(about));
	}
	if let Some(author) = string_field(map, "author") {
		command = command.author(leak(author));
	}
	add_args(&mut command, map.get(Value::String("args".into())));
	add_subcommands(&mut command, map.get(Value::String("subcommands".into())));
	command
}

fn add_args(command: &mut Command, value: Option<&Value>) {
	let Some(sequence) = value.and_then(Value::as_sequence) else {
		return;
	};
	for item in sequence {
		let Some(mapping) = item.as_mapping() else {
			continue;
		};
		for (name, properties) in mapping {
			let Some(name) = name.as_str() else {
				continue;
			};
			let Some(properties) = properties.as_mapping() else {
				continue;
			};
			let mut arg = Arg::new(leak(name.to_string()));
			if let Some(help) = string_field(properties, "help") {
				arg = arg.help(leak(help));
			}
			if let Some(long) = string_field(properties, "long") {
				arg = arg.long(leak(long.trim_end_matches(',').to_string()));
			}
			if let Some(short) = string_field(properties, "short") {
				if let Some(ch) = short.trim().chars().next() {
					arg = arg.short(ch);
				}
			}
			if let Some(default) = string_field(properties, "default_value") {
				arg = arg.default_value(leak(default));
			}
			if let Some(index) = properties
				.get(Value::String("index".into()))
				.and_then(Value::as_u64)
			{
				arg = arg.index(index as usize);
			}
			if let Some(values) = properties
				.get(Value::String("possible_values".into()))
				.and_then(Value::as_sequence)
			{
				let values: Vec<&'static str> = values
					.iter()
					.filter_map(Value::as_str)
					.map(|value| leak(value.to_string()))
					.collect();
				if !values.is_empty() {
					arg = arg.value_parser(clap::builder::PossibleValuesParser::new(values));
				}
			}
			let takes_value = properties
				.get(Value::String("takes_value".into()))
				.and_then(Value::as_bool)
				.unwrap_or(false);
			if takes_value {
				arg = arg.num_args(1);
			} else {
				arg = arg.action(ArgAction::SetTrue);
			}
			*command = command.clone().arg(arg);
		}
	}
}

fn add_subcommands(command: &mut Command, value: Option<&Value>) {
	let Some(sequence) = value.and_then(Value::as_sequence) else {
		return;
	};
	for item in sequence {
		let Some(mapping) = item.as_mapping() else {
			continue;
		};
		for (name, definition) in mapping {
			let Some(name) = name.as_str() else {
				continue;
			};
			let Some(definition) = definition.as_mapping() else {
				continue;
			};
			let mut sub = Command::new(leak(name.to_string()));
			if let Some(about) = string_field(definition, "about") {
				sub = sub.about(leak(about));
			}
			add_args(&mut sub, definition.get(Value::String("args".into())));
			add_subcommands(
				&mut sub,
				definition.get(Value::String("subcommands".into())),
			);
			*command = command.clone().subcommand(sub);
		}
	}
}

fn string_field(mapping: &Mapping, key: &str) -> Option<String> {
	mapping
		.get(Value::String(key.to_string()))
		.and_then(Value::as_str)
		.map(str::to_string)
}
