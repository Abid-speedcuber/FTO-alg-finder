use std::{env, fs, io::Read, process};

use fto_core::{
    search::{self, SearchConfig},
    tables::TransitionTables,
    FtoCubie,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut max_depth = 6_u8;
    let mut exact_depth = false;
    let mut find_all = false;
    let mut json_path = None;
    let mut scramble = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--depth" => {
                i += 1;
                max_depth = args
                    .get(i)
                    .ok_or("--depth needs a value")?
                    .parse()
                    .map_err(|_| "--depth must be an integer from 0 to 255")?;
            }
            "--all" => find_all = true,
            "--exact" => exact_depth = true,
            "--json" => {
                i += 1;
                json_path = Some(args.get(i).ok_or("--json needs a file path")?.clone());
            }
            "--scramble" => {
                i += 1;
                scramble = Some(args.get(i).ok_or("--scramble needs a quoted move sequence")?.clone());
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 1;
    }

    let cubie = if let Some(sequence) = scramble {
        apply_sequence(FtoCubie::solved(), &sequence)?
    } else {
        let input = if let Some(path) = json_path {
            fs::read_to_string(path).map_err(|error| error.to_string())?
        } else {
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .map_err(|error| error.to_string())?;
            input
        };
        if input.trim().is_empty() {
            FtoCubie::solved()
        } else {
            parse_cubie_json(&input)?
        }
    };

    eprintln!("loading transition tables...");
    let tables = TransitionTables::load_or_build("cache/transition-tables-v1.bin")
        .map_err(|error| error.to_string())?;
    let result = search::solve(
        cubie.coord(),
        &tables,
        &SearchConfig {
            min_depth: if exact_depth { max_depth } else { 0 },
            max_depth,
            find_all,
        },
    );

    println!("nodes: {}", result.nodes);
    println!("solutions: {}", result.solutions.len());
    for solution in result.solutions {
        println!("{}", search::format_solution(&solution));
    }
    Ok(())
}

fn print_help() {
    println!(
        "Usage:
  fto-cli --scramble \"R U R'\" --depth 6
  fto-cli --json state.json --depth 6 --all --exact
  fto-cli --depth 6 --all --exact < state.json

If no state is provided, the solved state is used."
    );
}

fn parse_cubie_json(input: &str) -> Result<FtoCubie, String> {
    Ok(FtoCubie::new(
        parse_array::<6>(input, "cp", 0, 5)?,
        parse_array::<6>(input, "co", 0, 1)?,
        parse_array::<12>(input, "ep", 0, 11)?,
        parse_array::<12>(input, "uf", 0, 11)?,
        parse_array::<12>(input, "rl", 0, 11)?,
    ))
}

fn parse_array<const N: usize>(
    input: &str,
    key: &str,
    min: u8,
    max: u8,
) -> Result<[u8; N], String> {
    let needle = format!("\"{key}\"");
    let key_start = input
        .find(&needle)
        .ok_or_else(|| format!("missing JSON key: {key}"))?;
    let after_key = &input[key_start + needle.len()..];
    let open = after_key
        .find('[')
        .ok_or_else(|| format!("{key} must be a JSON array"))?;
    let after_open = &after_key[open + 1..];
    let close = after_open
        .find(']')
        .ok_or_else(|| format!("{key} array is missing closing bracket"))?;
    let body = &after_open[..close];
    let values = body
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .parse::<u8>()
                .map_err(|_| format!("{key} contains a non-u8 value: {value}"))
                .and_then(|value| {
                    if (min..=max).contains(&value) {
                        Ok(value)
                    } else {
                        Err(format!("{key} value {value} is outside {min}..={max}"))
                    }
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let out: [u8; N] = values
        .try_into()
        .map_err(|values: Vec<u8>| format!("{key} must contain {N} values, got {}", values.len()))?;
    Ok(out)
}

fn apply_sequence(mut cubie: FtoCubie, sequence: &str) -> Result<FtoCubie, String> {
    for token in sequence.split_whitespace() {
        let mv = parse_move(token)?;
        cubie = cubie.apply(mv);
    }
    Ok(cubie)
}

fn parse_move(token: &str) -> Result<fto_core::moves::Move, String> {
    use fto_core::moves::Move;

    match token {
        "U" => Ok(Move::U),
        "U'" | "Ui" => Ok(Move::Up),
        "B" => Ok(Move::B),
        "B'" | "Bi" => Ok(Move::Bp),
        "R" => Ok(Move::R),
        "R'" | "Ri" => Ok(Move::Rp),
        "L" => Ok(Move::L),
        "L'" | "Li" => Ok(Move::Lp),
        "Rw" => Ok(Move::Rw),
        "Rw'" | "Rwi" => Ok(Move::Rwp),
        _ => Err(format!("unknown move: {token}")),
    }
}
