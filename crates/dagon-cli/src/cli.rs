//! Every `dagon` command and option, with its help: parsing, `--help` and
//! usage errors come from these definitions, and `dagon help-markdown`
//! prints them as Markdown for the user documentation.

use clap::{Args, CommandFactory, Parser, Subcommand};

const CONFIG: &str = "Project file the defaults come from [default: ./dagon.toml]";
const TOPOLOGY: &str = "Topology YAML [default: topology in dagon.toml]";
const HUB_PORT: &str =
    "Serial port of the hub [default: the USB device c0de:cafe or named \"DAGON hub\"]";
const KEY: &str = "Secret signing key file, outside any git work tree \
                   [default: $DAGON_SIGNING_KEY]";
const PUBKEY: &str = "Public key file the release is verified with \
                      [default: pubkey in dagon.toml, else the public half of the signing key]";
const NODES: &str = "Node ids [default: every [build.<id>] of dagon.toml]";
const LISTEN_PORT: &str = "TCP port on 127.0.0.1 [default: a free port]";
const NO_OPEN: &str = "Do not open the browser; the address and passphrase are printed";

const AFTER_HELP: &str = "\
Most commands take their defaults from ./dagon.toml (or --config <file>).
The hub is found by USB ID c0de:cafe or product \"DAGON hub\" unless --port is given.
The secret key file is --key <file> or $DAGON_SIGNING_KEY and must lie outside any git
work tree.

`dagon <command> --help` describes a command and each of its options.";

/// DAGON Boot command line tool
#[derive(Debug, Parser)]
#[command(
    name = "dagon",
    bin_name = "dagon",
    disable_version_flag = true,
    args_override_self = true,
    args_conflicts_with_subcommands = true,
    after_help = AFTER_HELP
)]
pub struct Cli {
    /// Print the version and the format numbers this dagon reads and writes
    #[arg(short = 'V', long)]
    pub version: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, PartialEq, Subcommand)]
pub enum Command {
    /// Make or show the signing key pair
    #[command(subcommand)]
    Key(KeyCmd),
    /// Check, compile or send a topology
    #[command(subcommand)]
    Topo(TopoCmd),
    /// Sign firmware images
    #[command(subcommand)]
    Image(ImageCmd),
    /// Build a signed release package (.dgb)
    #[command(subcommand)]
    Release(ReleaseCmd),
    /// Make the first-install files of a node from [provision.<node>] in dagon.toml
    Provision(ProvisionArgs),
    /// Make the hub's rescue UF2 (boot2, stage0, logs, boot core and app)
    Rescue(RescueArgs),
    /// Show HELLO, STATUS and VERSION of the hub and each topology node
    ///
    /// Versions of stage0, boot core, app, wire format and MCU.
    Status(StatusArgs),
    /// Write a release package to the nodes
    ///
    /// Verifies the release, checks it matches the topology, sends the topology
    /// to the hub, then updates the children and then the hub.
    Apply(ApplyArgs),
    /// Run the 12 conformance tests on a node
    ///
    /// Runs through the hub, or with --usb on a node plugged in directly. The
    /// images folder holds good-a.img, good-b.img, no-confirm-a.img and
    /// no-confirm-b.img. Writes result.json and report.html into --out, else
    /// into the images folder (default for both: out/conform/<id> next to
    /// dagon.toml).
    Conform(ConformArgs),
    /// Go back to the node's previously confirmed version
    ///
    /// Boots the node's other slot and confirms it there.
    Rollback(RollbackArgs),
    /// Make a project folder
    ///
    /// Writes dagon.toml and topology.yaml (the sample unless --from), and a
    /// signing key pair in --keys if none is there. The project takes DAGON
    /// from this CLI's release (git and tag cli-v<version>) unless
    /// --dagon-path or DAGON_HOME names a local clone.
    New(NewArgs),
    /// Make one Rust crate per node in nodes/<id> on the board support packages
    ///
    /// Rewrites only wiring.rs and the package line, never app.rs (main.rs
    /// only with --force-main).
    Gen(GenArgs),
    /// Build each node of [build.<id>] in dagon.toml
    ///
    /// Writes out/build/<id>/app-a.elf (and app-b.elf).
    Build(BuildArgs),
    /// Build, sign, package and write the nodes in one go
    ///
    /// Each node's version is its crate's Cargo.toml version. Lists node and
    /// version and asks first; keeps the package in out/deploy. Each node's
    /// boot core comes from its boot crate and is written (before its app) only
    /// when its version differs from the running boot core's.
    Deploy(DeployArgs),
    /// First write of a node, as its MCU definition says
    ///
    /// Writes through a UF2 drive, probe-rs or espflash. Builds the parts (boot
    /// core, stage0, factory agent) with the key and prints the plan first.
    /// Writes to the one device the writer reports, or the one you choose or
    /// name.
    Flash(FlashArgs),
    /// Check the tools each MCU family of the topology needs
    ///
    /// Then shows each node's versions when a hub is connected.
    Doctor(DoctorArgs),
    /// Start DAGON Console, the browser tool, and its API on 127.0.0.1
    ///
    /// Opens it with a one-time passphrase. The Console verifies releases with
    /// the public key given here (--pubkey, pubkey in dagon.toml, or the
    /// signing key's public half).
    Serve(ServeArgs),
    /// Run the simulator
    #[command(subcommand)]
    Sim(SimCmd),
    /// Work with an AI agent
    #[command(subcommand)]
    Ai(AiCmd),
    /// Same as --version
    #[command(hide = true)]
    Version,
    /// Every command's help as Markdown
    #[command(hide = true, name = "help-markdown")]
    HelpMarkdown,
}

#[derive(Debug, PartialEq, Subcommand)]
pub enum KeyCmd {
    /// Generate a signing key pair
    Gen {
        /// Folder for the key pair; must lie outside any git work tree
        #[arg(long, value_name = "DIR")]
        out: String,
    },
    /// Print the public key (hex) and key id of the signing key
    Pub {
        #[arg(long, value_name = "FILE", help = KEY)]
        key: Option<String>,
    },
}

#[derive(Debug, PartialEq, Subcommand)]
pub enum TopoCmd {
    /// Validate a topology file
    Check(TopoCheckArgs),
    /// Write the topology's canonical binary
    Compile(TopoCompileArgs),
    /// Send the topology's canonical binary to the hub
    Push(TopoPushArgs),
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct TopoCheckArgs {
    /// Print the report as JSON
    #[arg(long)]
    pub json: bool,
    /// Topology YAML
    pub yaml: String,
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct TopoCompileArgs {
    /// Topology YAML
    pub yaml: String,
    /// Output file [default: the YAML's name with .bin]
    #[arg(long, value_name = "BIN")]
    pub out: Option<String>,
    #[arg(long, value_name = "FILE", help = CONFIG)]
    pub config: Option<String>,
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct TopoPushArgs {
    #[arg(value_name = "YAML", help = TOPOLOGY)]
    pub yaml: Option<String>,
    #[arg(long, value_name = "NAME", help = HUB_PORT)]
    pub port: Option<String>,
    #[arg(long, value_name = "FILE", help = CONFIG)]
    pub config: Option<String>,
}

#[derive(Debug, PartialEq, Subcommand)]
pub enum ImageCmd {
    /// Build and sign an image for a topology node
    ///
    /// An input that already is a signed image is verified and checked to
    /// belong to the node, kind and slot instead.
    Sign(ImageSignArgs),
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct ImageSignArgs {
    /// ELF or raw binary to sign
    #[arg(value_name = "ELF|BIN")]
    pub input: String,
    /// Node id in the topology
    #[arg(long, value_name = "ID")]
    pub node: String,
    /// Version in the image header
    #[arg(long, value_name = "X.Y.Z")]
    pub version: String,
    /// Output image file
    #[arg(long, value_name = "IMG")]
    pub out: String,
    /// Slot the image is built for: a or b
    #[arg(long, value_name = "SLOT", default_value = "a")]
    pub slot: String,
    /// Image kind: app, bootcore or agent
    #[arg(long, value_name = "KIND", default_value = "app")]
    pub kind: String,
    /// Release sequence number
    #[arg(long, value_name = "N", default_value = "1")]
    pub seq: String,
    /// Oldest boot core version the image runs on [default: 0.0.0]
    #[arg(long, value_name = "X.Y.Z")]
    pub min_bootcore: Option<String>,
    /// Protocol version
    #[arg(long, value_name = "N", default_value = "1")]
    pub proto: String,
    #[arg(long, value_name = "LIST", help = names_help(
        "Build flags, comma-separated [default: the kind's]",
        dagon_sign::image::BUILD_FLAG_NAMES,
    ))]
    pub flags: Option<String>,
    #[arg(long, value_name = "LIST", help = names_help(
        "Capabilities, comma-separated [default: the kind's]",
        dagon_sign::image::CAPABILITY_NAMES,
    ))]
    pub caps: Option<String>,
    #[arg(long, value_name = "YAML", help = TOPOLOGY)]
    pub topology: Option<String>,
    #[arg(long, value_name = "FILE", help = KEY)]
    pub key: Option<String>,
    #[arg(long, value_name = "FILE", help = CONFIG)]
    pub config: Option<String>,
}

fn names_help(what: &str, names: &[(&str, u32)]) -> String {
    let list: Vec<&str> = names.iter().map(|(n, _)| *n).collect();
    format!("{what}: {}", list.join(", "))
}

#[derive(Debug, PartialEq, Subcommand)]
pub enum ReleaseCmd {
    /// Check the images, sign the manifest and write the release package
    Build(ReleaseBuildArgs),
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct ReleaseBuildArgs {
    /// Signed images, or folders whose .img files are taken
    #[arg(value_name = "IMG|DIR", required = true)]
    pub inputs: Vec<String>,
    /// Release name in the manifest
    #[arg(long, value_name = "NAME")]
    pub name: String,
    /// Output package [default: release in dagon.toml]
    #[arg(long, value_name = "DGB")]
    pub out: Option<String>,
    #[arg(long, value_name = "YAML", help = TOPOLOGY)]
    pub topology: Option<String>,
    /// Topology binary to package [default: compiled from the topology]
    #[arg(long, value_name = "BIN")]
    pub topology_bin: Option<String>,
    #[arg(long, value_name = "FILE", help = KEY)]
    pub key: Option<String>,
    #[arg(long, value_name = "FILE", help = CONFIG)]
    pub config: Option<String>,
}

/// Options of the first-install files (`provision`, `rescue`).
#[derive(Debug, Default, PartialEq, Args)]
pub struct FirstInstallOpts {
    /// Output folder [default: <provision_dir>/<node>]
    #[arg(long, value_name = "DIR")]
    pub out: Option<String>,
    #[arg(long, value_name = "YAML", help = TOPOLOGY)]
    pub topology: Option<String>,
    #[arg(long, value_name = "FILE", help = KEY)]
    pub key: Option<String>,
    #[arg(long, value_name = "FILE", help = CONFIG)]
    pub config: Option<String>,
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct ProvisionArgs {
    /// Node id in the topology
    pub node: String,
    #[command(flatten)]
    pub files: FirstInstallOpts,
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct RescueArgs {
    /// What to rescue (the topology's coordinator)
    #[arg(value_parser = ["hub"])]
    pub target: String,
    #[command(flatten)]
    pub files: FirstInstallOpts,
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct StatusArgs {
    #[arg(long, value_name = "NAME", help = HUB_PORT)]
    pub port: Option<String>,
    #[arg(long, value_name = "YAML", help = TOPOLOGY)]
    pub topology: Option<String>,
    #[arg(long, value_name = "FILE", help = CONFIG)]
    pub config: Option<String>,
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct ApplyArgs {
    /// Release package [default: release in dagon.toml]
    #[arg(value_name = "DGB")]
    pub dgb: Option<String>,
    /// Update only this node (repeat for more) [default: every node]
    #[arg(long, value_name = "ID")]
    pub node: Vec<String>,
    #[arg(long, value_name = "NAME", help = HUB_PORT)]
    pub port: Option<String>,
    #[arg(long, value_name = "YAML", help = TOPOLOGY)]
    pub topology: Option<String>,
    #[arg(long, value_name = "FILE", help = PUBKEY)]
    pub pubkey: Option<String>,
    #[arg(long, value_name = "FILE", help = KEY)]
    pub key: Option<String>,
    /// Leave signature and digest checks to the nodes (for demonstration only)
    #[arg(long)]
    pub no_host_verify: bool,
    #[arg(long, value_name = "FILE", help = CONFIG)]
    pub config: Option<String>,
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct ConformArgs {
    /// Node id in the topology
    #[arg(long, value_name = "ID")]
    pub node: String,
    /// Test a node plugged in directly over USB instead of through the hub
    #[arg(long)]
    pub usb: bool,
    /// Folder of the test images [default: out/conform/<id>]
    #[arg(long, value_name = "DIR")]
    pub images: Option<String>,
    /// Folder for result.json and report.html [default: the images folder]
    #[arg(long, value_name = "DIR")]
    pub out: Option<String>,
    #[arg(long, value_name = "NAME", help = HUB_PORT)]
    pub port: Option<String>,
    #[arg(long, value_name = "YAML", help = TOPOLOGY)]
    pub topology: Option<String>,
    #[arg(long, value_name = "FILE", help = KEY)]
    pub key: Option<String>,
    #[arg(long, value_name = "FILE", help = CONFIG)]
    pub config: Option<String>,
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct RollbackArgs {
    /// Node id in the topology
    #[arg(long, value_name = "ID")]
    pub node: String,
    #[arg(long, value_name = "NAME", help = HUB_PORT)]
    pub port: Option<String>,
    #[arg(long, value_name = "YAML", help = TOPOLOGY)]
    pub topology: Option<String>,
    #[arg(long, value_name = "FILE", help = CONFIG)]
    pub config: Option<String>,
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct NewArgs {
    /// Project folder to make; its name is the project's
    pub dir: String,
    /// Topology YAML to start from [default: the sample topology]
    #[arg(long, value_name = "YAML")]
    pub from: Option<String>,
    /// Folder of the signing key pair [default: ~/.dagon/keys/<name>]
    #[arg(long, value_name = "DIR")]
    pub keys: Option<String>,
    /// Local clone of DAGON to build with [default: $DAGON_HOME]
    #[arg(long, value_name = "CLONE")]
    pub dagon_path: Option<String>,
    /// Git URL of DAGON [default: this CLI's repository]
    #[arg(long, value_name = "URL")]
    pub dagon_git: Option<String>,
    /// Git tag of DAGON [default: cli-v<this CLI's version>]
    #[arg(long, value_name = "TAG")]
    pub dagon_tag: Option<String>,
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct GenArgs {
    /// Node ids [default: every node]
    #[arg(value_name = "ID")]
    pub nodes: Vec<String>,
    /// Also rewrite main.rs
    #[arg(long)]
    pub force_main: bool,
    #[arg(long, value_name = "FILE", help = CONFIG)]
    pub config: Option<String>,
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct BuildArgs {
    #[arg(value_name = "ID", help = NODES)]
    pub nodes: Vec<String>,
    #[arg(long, value_name = "FILE", help = CONFIG)]
    pub config: Option<String>,
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct DeployArgs {
    #[arg(value_name = "ID", help = NODES)]
    pub nodes: Vec<String>,
    /// Version to deploy; must equal each node crate's version
    #[arg(long, value_name = "X.Y.Z")]
    pub version: Option<String>,
    /// Write without asking, the ROM question of a silent node included
    #[arg(long)]
    pub yes: bool,
    /// Refuse a node with boot core slots that has no boot crate
    #[arg(long)]
    pub bootcore: bool,
    #[arg(long, value_name = "NAME", help = HUB_PORT)]
    pub port: Option<String>,
    #[arg(long, value_name = "YAML", help = TOPOLOGY)]
    pub topology: Option<String>,
    #[arg(long, value_name = "FILE", help = KEY)]
    pub key: Option<String>,
    #[arg(long, value_name = "FILE", help = CONFIG)]
    pub config: Option<String>,
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct FlashArgs {
    /// Node id in the topology
    pub node: String,
    /// Make the files and print the plan without writing
    #[arg(long)]
    pub dry_run: bool,
    /// Write without asking
    #[arg(long)]
    pub yes: bool,
    /// Debug probe to write through (probe-rs writers)
    #[arg(long, value_name = "VID:PID[:SERIAL]")]
    pub probe: Option<String>,
    /// Serial port to write through (espflash writers)
    #[arg(long, value_name = "PORT")]
    pub port: Option<String>,
    /// UF2 drive to copy to (UF2 writers)
    #[arg(long, value_name = "PATH")]
    pub drive: Option<String>,
    /// Folder for the files [default: <provision_dir>/<node>]
    #[arg(long, value_name = "DIR")]
    pub out: Option<String>,
    #[arg(long, value_name = "YAML", help = TOPOLOGY)]
    pub topology: Option<String>,
    #[arg(long, value_name = "FILE", help = KEY)]
    pub key: Option<String>,
    #[arg(long, value_name = "FILE", help = CONFIG)]
    pub config: Option<String>,
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct DoctorArgs {
    /// Install what needs no administrator rights
    #[arg(long)]
    pub fix: bool,
    #[arg(long, value_name = "NAME", help = HUB_PORT)]
    pub port: Option<String>,
    /// Topology YAML [default: topology in dagon.toml, else every MCU family]
    #[arg(long, value_name = "YAML")]
    pub topology: Option<String>,
    #[arg(long, value_name = "FILE", help = CONFIG)]
    pub config: Option<String>,
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct ServeArgs {
    #[arg(long, value_name = "N", help = LISTEN_PORT)]
    pub port: Option<String>,
    /// Folder of the Console page to serve [default: the page built into dagon]
    #[arg(long, value_name = "DIR")]
    pub web: Option<String>,
    #[arg(long, value_name = "FILE", help = PUBKEY)]
    pub pubkey: Option<String>,
    #[arg(long, value_name = "FILE", help = KEY)]
    pub key: Option<String>,
    #[arg(long, help = NO_OPEN)]
    pub no_open: bool,
    #[arg(long, value_name = "FILE", help = CONFIG)]
    pub config: Option<String>,
}

#[derive(Debug, PartialEq, Subcommand)]
pub enum SimCmd {
    /// Run the simulator scenarios (no hardware)
    Run,
}

#[derive(Debug, PartialEq, Subcommand)]
pub enum AiCmd {
    /// Link DAGON to an agent one way, with your consent
    ///
    /// Agents are TOML files, built in or in ~/.dagon/agents. Shows where it
    /// sends and what, and asks your consent once. Logins stay with the agent;
    /// API keys go in the OS credential store.
    Link(AgentWay),
    /// Unlink DAGON and withdraw the consent
    Unlink,
    /// Manage an agent's API key in the OS credential store
    ///
    /// Windows Credential Manager, macOS Keychain or Secret Service.
    #[command(subcommand)]
    Key(AiKeyCmd),
    /// Ask DAGON about the topology of ./dagon.toml
    ///
    /// A proposed change is shown as a diff, never applied.
    Ask(AskArgs),
    /// Turn a board description into topology YAML
    ///
    /// The description comes from --in or stdin; the YAML is checked to 0
    /// errors. Needs `dagon ai link` first.
    Topology(AiTopologyArgs),
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct AgentWay {
    /// Agent id [default: the first agent]
    #[arg(long, value_name = "ID")]
    pub agent: Option<String>,
    /// Way id of the agent [default: its first way]
    #[arg(long, value_name = "ID")]
    pub way: Option<String>,
}

#[derive(Debug, PartialEq, Subcommand)]
pub enum AiKeyCmd {
    /// Type the key in; it is read without echo
    Set(AgentWay),
    /// Show registered or not for every way that takes a key
    Status,
    /// Delete the key
    Delete(AgentWay),
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct AskArgs {
    /// The question, in one or more words
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    pub question: Vec<String>,
}

#[derive(Debug, Default, PartialEq, Args)]
pub struct AiTopologyArgs {
    /// Board description file [default: stdin]
    #[arg(long = "in", value_name = "FILE")]
    pub input: Option<String>,
    /// Output topology YAML
    #[arg(long, value_name = "YAML", default_value = "topology.yaml")]
    pub out: String,
    /// Model id to ask [default: the agent's]
    #[arg(long, value_name = "ID")]
    pub model: Option<String>,
}

/// Every visible command's full help, one Markdown section each.
pub fn markdown() -> String {
    let mut root = Cli::command();
    root.build();
    let mut out = String::new();
    section(&root, &mut out);
    out
}

fn section(cmd: &clap::Command, out: &mut String) {
    let name = cmd.get_bin_name().unwrap_or(cmd.get_name());
    let help = cmd.clone().render_long_help().to_string();
    out.push_str(&format!(
        "## {name}\n\n```text\n{}\n```\n\n",
        help.trim_end()
    ));
    for sub in cmd
        .get_subcommands()
        .filter(|c| !c.is_hide_set() && c.get_name() != "help")
    {
        section(sub, out);
    }
}

/// `argv` (without the command words) parsed as one command's options, for
/// tests that run a command function directly.
#[cfg(test)]
pub fn args<T: Args + clap::FromArgMatches>(argv: &[&str]) -> T {
    let cmd = T::augment_args(clap::Command::new("dagon").args_override_self(true));
    let m = cmd.get_matches_from(std::iter::once("dagon").chain(argv.iter().copied()));
    T::from_arg_matches(&m).expect("options")
}

#[cfg(test)]
mod tests;
