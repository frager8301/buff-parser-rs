use colored::*;
use futures::stream::{self, StreamExt};
use regex::Regex;
use rust_translate::translate;
use serde_json::Value;
use std::fs::File;
use std::io::BufWriter;
use std::io::{self, BufRead, BufReader, Read, Write, stdout};
use std::path::Path;
use tokio::time::{Duration, sleep};
use crossterm::{execute, terminal::{Clear, ClearType}};

fn clear() {
    execute!(stdout(), Clear(ClearType::All)).unwrap();
}

fn set_title(title: &str) {
    print!("\x1b]0;{}\x07", title);
    stdout().flush().unwrap();
}

const CONCURRENT_REQUESTS: usize = 1000;
const REQUEST_DELAY: u64 = 500;
const RETRY_ATTEMPTS: u32 = 3;

async fn translate_with_retry(text: &str, target_lang: &str) -> String {
    let mut attempts = 0;
    let original_text = text.replace("%", "percent");

    loop {
        match translate(&original_text, "zh-TW", target_lang).await {
            Ok(translated) => return translated.replace("percent", "%"),
            Err(e) if attempts < RETRY_ATTEMPTS => {
                let delay_secs = 2u64.pow(attempts);
                eprintln!(
                    "Retry {} for '{}...' (Error: {}), waiting {}s",
                    attempts + 1,
                    &text.chars().take(20).collect::<String>(),
                    e,
                    delay_secs
                );
                sleep(Duration::from_secs(delay_secs)).await;
                attempts += 1;
            }
            Err(e) => {
                eprintln!(
                    "Failed to translate after {} attempts: {}",
                    RETRY_ATTEMPTS, e
                );
                return original_text.replace("percent", "%");
            }
        }
    }
}

#[tokio::main]
async fn main() {
    set_title("BUFF-PARSER-RS");
    clear();

    let parse_all_buffs = loop {
        println!(
                    "{}",
                    "1) All Buffs\n2) Character Specific Buffs".bright_yellow()
        );
        let mut choice = String::new();
        io::stdin()
            .read_line(&mut choice)
            .expect("Failed to read choice");
        clear();

        match choice.trim() {
            "1" => break true,
            "2" => break false,
            _ => {
                println!("{}", "Invalid choice, please enter 1 or 2".bright_red());
                continue;
            }
        }
    };

    let mut role_ids: Vec<String> = vec![];
    let mut manual_ids: Vec<String> = vec![];

    if !parse_all_buffs {
        println!(
            "{}",
            "Enter IDs, separated by commas (e.g., 1407, 1507):".bright_yellow()
        );
        let mut input_ids = String::new();
        io::stdin()
            .read_line(&mut input_ids)
            .expect("Error reading input");
        clear();

        manual_ids = input_ids
            .trim()
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();

        role_ids = manual_ids.clone();
    }

    println!(
        "Enter filenames (including extensions, e.g., file.txt, file.json), separated by commas:"
    );

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Error reading input");
    clear();

    let filenames: Vec<String> = input
        .trim()
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    let re = Regex::new(r"Id:\s*(\d+)\s*\(1\)").expect("Invalid regex");
    let mut output_buffer = String::new();
    let mut total_buffs = 0;

    for filename in filenames {
        println!("{}", format!("Parse from: {}", filename).bright_blue());

        let path = Path::new(&filename);

        if !path.exists() {
            eprintln!("{}", format!("File not found: {}", filename).bright_red());
            continue;
        }

        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let mut output_lines: Vec<String> = Vec::new();

        match extension.as_str() {
            "txt" => {
                println!("{}", "Parse TXT file".bright_cyan());
                let file = File::open(&path).expect("Error opening file");
                let reader = BufReader::new(file);

                for line in reader.lines() {
                    if let Ok(line_content) = line {
                        if let Some(caps) = re.captures(&line_content) {
                            if let Some(id_match) = caps.get(1) {
                                let id = id_match.as_str();
                                if parse_all_buffs
                                    || role_ids.iter().any(|prefix| id.starts_with(prefix))
                                {
                                    output_lines.push(format!("{},", id));
                                }
                            }
                        }
                    }
                }
            }

            "json" => {
                println!("{}", "Parse JSON file".bright_magenta());
                let mut file = File::open(&path).expect("Error opening file");
                let mut contents = String::new();
                file.read_to_string(&mut contents)
                    .expect("Error reading input");

                if let Ok(json_data) = serde_json::from_str::<Value>(&contents) {
                    if let Some(array) = json_data.as_array() {
                        println!(
                            "{}",
                            format!("Records found: {}", array.len()).bright_white()
                        );

                        println!(
                            "{}",
                            "Enter target language code (e.g., en, fr, de):".bright_yellow()
                        );
                        let mut target_lang = String::new();
                        io::stdin()
                            .read_line(&mut target_lang)
                            .expect("Failed to read language input.");
                        let target_lang = target_lang.trim().to_lowercase();
                        clear();

                        let mut translation_futures = Vec::new();

                        for (index, entry) in array.iter().enumerate() {
                            if let Some(id_value) = entry.get("Id") {
                                if let Some(id_number) = id_value.as_u64() {
                                    let id_str = id_number.to_string();

                                    if parse_all_buffs
                                        || role_ids.iter().any(|prefix| id_str.starts_with(prefix))
                                    {
                                        let ge_desc = entry
                                            .get("GeDesc")
                                            .and_then(Value::as_str)
                                            .unwrap_or_default();

                                        let dur_policy = entry
                                            .get("DurationPolicy")
                                            .and_then(Value::as_u64)
                                            .unwrap_or(0);

                                        let id_clone = id_str.clone();
                                        let ge_clone = ge_desc.to_string();
                                        let target_lang_clone = target_lang.clone();

                                        translation_futures.push(async move {
                                            if index % 1500 == 0 {
                                                sleep(Duration::from_millis(REQUEST_DELAY)).await;
                                            }

                                            let translated =
                                                translate_with_retry(&ge_clone, &target_lang_clone)
                                                    .await;

                                            (id_clone, dur_policy, ge_clone, translated)
                                        });

                                        total_buffs += 1;
                                    }
                                }
                            }
                        }

                        println!("Starting translation of {} entries...", total_buffs);

                        let results = stream::iter(translation_futures)
                            .buffer_unordered(CONCURRENT_REQUESTS)
                            .collect::<Vec<_>>()
                            .await;

                        for result in results {
                            output_lines.push(format!(
                                "{} ({}), {} // Translated: {}",
                                result.0, result.1, result.2, result.3
                            ));
                        }
                    }
                }
            }
            _ => {
                eprintln!(
                    "{}",
                    format!("Unsupported file format: {}", filename).bright_yellow()
                );
                continue;
            }
        }

        output_buffer.push_str(&format!("{}:\n", filename));
        println!("\n{}:", filename.bright_cyan().bold());

        if output_lines.is_empty() {
            output_buffer.push_str("No matching entries found.\n");
            println!("{}", "No matching entries found.".dimmed());
        } else {
            output_buffer.push_str(&output_lines.join("\n"));
            output_buffer.push('\n');
            for line in &output_lines {
                println!("{}", line.bright_red());
            }
            output_buffer.push_str(&format!("\nTotal buffs: {}", total_buffs));
        }
    }

    let output_filename = if parse_all_buffs {
        "all_buffs.md".to_string()
    } else if !manual_ids.is_empty() {
        format!("{}.md", manual_ids.join("_"))
    } else if !role_ids.is_empty() {
        "buff.md".to_string()
    } else {
        "output.md".to_string()
    };

    let path = Path::new(&output_filename);
    let file =
        File::create(path).unwrap_or_else(|_| panic!("Failed to create file: {}", output_filename));
    let mut writer = BufWriter::new(file);
    writer
        .write_all(output_buffer.as_bytes())
        .expect("Error writing to file.");
    writer.flush().expect("Buffer reset failed.");
    println!("\nSaved to {}", output_filename.bright_green());

    println!("\nPress Enter to exit...");
    let _ = io::stdin().read_line(&mut String::new());
}
