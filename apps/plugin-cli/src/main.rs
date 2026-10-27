use std::collections::VecDeque;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use info_panel_plugin_cli::{
    NoTrustRoots, PackageError, PackageLimits, RunnerVerificationMode, VerificationMode,
    create_plugin_scaffold, pack_directory, read_config_file, read_signature_file,
    read_verified_lua_runner, read_verified_package, test_lua_package, test_wasm_package,
};
use panel_plugin_api::{PluginPermissions, PluginRuntimeKind};

fn main() -> ExitCode {
    match run(env::args_os().skip(1).map(PathBuf::from).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("plugin-cli: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(mut args: VecDeque<PathBuf>) -> Result<(), CliError> {
    let command = take_string(&mut args, "command")?;
    match command.as_str() {
        "new" => command_new(args),
        "validate" => command_validate(args),
        "test" => command_test(args),
        "pack" => command_pack(args),
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        _ => Err(CliError::Usage(format!("unknown command {command}"))),
    }
}

fn command_new(mut args: VecDeque<PathBuf>) -> Result<(), CliError> {
    let destination = take_path(&mut args, "destination")?;
    let mut id = None;
    let mut name = None;
    let mut source = None;
    let mut runtime = PluginRuntimeKind::LuaV1;
    while let Some(flag) = args.pop_front() {
        match flag.to_str() {
            Some("--id") => id = Some(take_string(&mut args, "plugin id")?),
            Some("--name") => name = Some(take_string(&mut args, "plugin name")?),
            Some("--source") => source = Some(take_string(&mut args, "source URL")?),
            Some("--runtime") => {
                runtime = match take_string(&mut args, "runtime")?.as_str() {
                    "lua" | "lua@1" => PluginRuntimeKind::LuaV1,
                    "wasm" | "wasm@1" => PluginRuntimeKind::WasmV1,
                    value => return Err(CliError::Usage(format!("unsupported runtime {value}"))),
                }
            }
            _ => return Err(CliError::Usage("unexpected new argument".to_owned())),
        }
    }
    create_plugin_scaffold(
        &destination,
        &id.ok_or_else(|| CliError::Usage("new requires --id".to_owned()))?,
        &name.ok_or_else(|| CliError::Usage("new requires --name".to_owned()))?,
        &source.ok_or_else(|| CliError::Usage("new requires --source".to_owned()))?,
        runtime,
    )?;
    println!(
        "created Info Panel plugin scaffold at {}",
        destination.display()
    );
    Ok(())
}

fn command_validate(args: VecDeque<PathBuf>) -> Result<(), CliError> {
    let ParsedVerification {
        package,
        signature,
        allow_unsigned_local,
        rest,
    } = parse_verification(args)?;
    if !rest.is_empty() {
        return Err(CliError::Usage("unexpected validate argument".to_owned()));
    }
    let signature_bytes = read_optional_signature(&signature)?;
    let mode = verification_mode(signature_bytes.as_deref(), allow_unsigned_local)?;
    let verified = read_verified_package(&package, PackageLimits::default(), &NoTrustRoots, mode)?;
    emit_warning(verified.warning())?;
    println!(
        "valid {} {} sha256={}",
        verified.manifest().id,
        verified.manifest().version,
        verified.sha256_hex()
    );
    Ok(())
}

fn command_test(args: VecDeque<PathBuf>) -> Result<(), CliError> {
    let ParsedVerification {
        package,
        signature,
        allow_unsigned_local,
        mut rest,
    } = parse_verification(args)?;
    let mut config = None;
    let mut lua_runner = None;
    let mut runner_signature = None;
    let mut runner_sha256 = None;
    let mut allow_local_runner = false;
    let mut grants = PluginPermissions::default();
    while let Some(flag) = rest.pop_front() {
        match flag.to_str() {
            Some("--config") => config = Some(take_path(&mut rest, "configuration")?),
            Some("--lua-runner") => lua_runner = Some(take_path(&mut rest, "Lua runner")?),
            Some("--runner-signature") => {
                runner_signature = Some(take_path(&mut rest, "runner signature")?)
            }
            Some("--runner-sha256") => {
                runner_sha256 = Some(parse_sha256(&take_string(&mut rest, "runner SHA-256")?)?)
            }
            Some("--allow-local-runner") => allow_local_runner = true,
            Some("--grant-network") => grants
                .network
                .push(take_string(&mut rest, "network origin")?),
            Some("--grant-cache") => grants.cache = true,
            Some("--grant-config") => grants.config = true,
            Some("--grant-locale") => grants.locale = true,
            Some("--grant-platform") => grants.platform = true,
            _ => return Err(CliError::Usage("unexpected test argument".to_owned())),
        }
    }
    let signature_bytes = read_optional_signature(&signature)?;
    let mode = verification_mode(signature_bytes.as_deref(), allow_unsigned_local)?;
    let verified = read_verified_package(&package, PackageLimits::default(), &NoTrustRoots, mode)?;
    emit_warning(verified.warning())?;
    let config = match config {
        Some(path) => read_config_file(&path)?,
        None => b"{}".to_vec(),
    };
    let result = match verified.manifest().runtime {
        PluginRuntimeKind::WasmV1 => {
            reject_runner_arguments(
                &lua_runner,
                &runner_signature,
                runner_sha256,
                allow_local_runner,
            )?;
            test_wasm_package(&verified, &config, &grants)?
        }
        PluginRuntimeKind::LuaV1 => {
            let runner_path = lua_runner
                .ok_or_else(|| CliError::Usage("test of lua@1 requires --lua-runner".to_owned()))?;
            let runner_signature_bytes = read_optional_signature(&runner_signature)?;
            let runner_mode = runner_verification_mode(
                runner_signature_bytes.as_deref(),
                runner_sha256,
                allow_local_runner,
            )?;
            let runner =
                read_verified_lua_runner(&runner_path, &verified, &NoTrustRoots, runner_mode)?;
            emit_warning(runner.warning())?;
            test_lua_package(&verified, &config, &runner, &grants)?
        }
    };
    println!(
        "tested {} page={} title={}",
        result.plugin_id, result.page_id, result.page_title
    );
    Ok(())
}

fn command_pack(mut args: VecDeque<PathBuf>) -> Result<(), CliError> {
    let source = take_path(&mut args, "source directory")?;
    let output = take_path(&mut args, "output package")?;
    let mut entry_file = None;
    while let Some(flag) = args.pop_front() {
        match flag.to_str() {
            Some("--entry-file") => entry_file = Some(take_path(&mut args, "entry file")?),
            _ => return Err(CliError::Usage("unexpected pack argument".to_owned())),
        }
    }
    let digest = pack_directory(
        &source,
        &output,
        entry_file.as_deref(),
        PackageLimits::default(),
    )?;
    println!("packed {} sha256={}", output.display(), encode_hex(&digest));
    Ok(())
}

struct ParsedVerification {
    package: PathBuf,
    signature: Option<PathBuf>,
    allow_unsigned_local: bool,
    rest: VecDeque<PathBuf>,
}

fn parse_verification(mut args: VecDeque<PathBuf>) -> Result<ParsedVerification, CliError> {
    let package = take_path(&mut args, "package")?;
    let mut signature = None;
    let mut allow_unsigned_local = false;
    let mut rest = VecDeque::new();
    while let Some(argument) = args.pop_front() {
        match argument.to_str() {
            Some("--signature") => signature = Some(take_path(&mut args, "signature")?),
            Some("--allow-unsigned-local") => allow_unsigned_local = true,
            _ => rest.push_back(argument),
        }
    }
    if signature.is_some() && allow_unsigned_local {
        return Err(CliError::Usage(
            "--signature and --allow-unsigned-local are mutually exclusive".to_owned(),
        ));
    }
    Ok(ParsedVerification {
        package,
        signature,
        allow_unsigned_local,
        rest,
    })
}

fn verification_mode(
    signature: Option<&[u8]>,
    allow_unsigned_local: bool,
) -> Result<VerificationMode<'_>, CliError> {
    match (signature, allow_unsigned_local) {
        (Some(signature), false) => Ok(VerificationMode::Signed(signature)),
        (None, true) => Ok(VerificationMode::AllowUnsignedLocal),
        (None, false) => Err(CliError::Usage(
            "unsigned packages require explicit --allow-unsigned-local development mode".to_owned(),
        )),
        (Some(_), true) => unreachable!("mutual exclusion checked while parsing"),
    }
}

fn runner_verification_mode(
    signature: Option<&[u8]>,
    digest: Option<[u8; 32]>,
    allow_local: bool,
) -> Result<RunnerVerificationMode<'_>, CliError> {
    let choices =
        usize::from(signature.is_some()) + usize::from(digest.is_some()) + usize::from(allow_local);
    if choices != 1 {
        return Err(CliError::Usage(
            "Lua runner requires exactly one of --runner-signature, --runner-sha256, or --allow-local-runner"
                .to_owned(),
        ));
    }
    if let Some(signature) = signature {
        Ok(RunnerVerificationMode::Signed(signature))
    } else if let Some(digest) = digest {
        Ok(RunnerVerificationMode::TrustedDigest(digest))
    } else {
        Ok(RunnerVerificationMode::AllowUnsignedLocal)
    }
}

fn reject_runner_arguments(
    path: &Option<PathBuf>,
    signature: &Option<PathBuf>,
    digest: Option<[u8; 32]>,
    allow_local: bool,
) -> Result<(), CliError> {
    if path.is_some() || signature.is_some() || digest.is_some() || allow_local {
        return Err(CliError::Usage(
            "Lua runner arguments are invalid for wasm@1".to_owned(),
        ));
    }
    Ok(())
}

fn read_optional_signature(path: &Option<PathBuf>) -> Result<Option<Vec<u8>>, CliError> {
    path.as_ref()
        .map(|path| read_signature_file(path))
        .transpose()
        .map_err(CliError::from)
}

fn emit_warning(
    warning: Option<&info_panel_plugin_cli::PersistentWarning>,
) -> Result<(), CliError> {
    if let Some(warning) = warning {
        let json = serde_json::to_string(warning).map_err(|_| CliError::WarningEncoding)?;
        eprintln!("{json}");
    }
    Ok(())
}

fn parse_sha256(value: &str) -> Result<[u8; 32], CliError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CliError::Usage(
            "runner SHA-256 must contain exactly 64 hexadecimal characters".to_owned(),
        ));
    }
    let mut digest = [0_u8; 32];
    for (index, output) in digest.iter_mut().enumerate() {
        *output = (hex_nibble(value.as_bytes()[index * 2])? << 4)
            | hex_nibble(value.as_bytes()[index * 2 + 1])?;
    }
    Ok(digest)
}

fn hex_nibble(byte: u8) -> Result<u8, CliError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(CliError::Usage("invalid hexadecimal digest".to_owned())),
    }
}

fn take_path(args: &mut VecDeque<PathBuf>, description: &str) -> Result<PathBuf, CliError> {
    args.pop_front()
        .ok_or_else(|| CliError::Usage(format!("missing {description}")))
}

fn take_string(args: &mut VecDeque<PathBuf>, description: &str) -> Result<String, CliError> {
    take_path(args, description)?
        .into_os_string()
        .into_string()
        .map_err(|_| CliError::Usage(format!("{description} must be valid UTF-8")))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn print_usage() {
    println!(
        "Info Panel plugin CLI\n\n\
         plugin-cli new <directory> --id <id> --name <name> --source <https-url> [--runtime lua|wasm]\n\
         plugin-cli validate <package.info-panel-plugin> (--signature <file> | --allow-unsigned-local)\n\
         plugin-cli test <package.info-panel-plugin> (--signature <file> | --allow-unsigned-local) [--config <file>] [--grant-network <origin>] [--grant-cache] [--grant-config] [--grant-locale] [--grant-platform] [--lua-runner <wasm> (--runner-signature <file> | --runner-sha256 <hex> | --allow-local-runner)]\n\
         plugin-cli pack <directory> <output.info-panel-plugin> [--entry-file <path>]"
    );
}

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("{0}")]
    Usage(String),
    #[error(transparent)]
    Package(#[from] PackageError),
    #[error("could not encode structured warning")]
    WarningEncoding,
}
