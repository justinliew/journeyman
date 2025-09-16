use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::Duration;
use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

#[derive(Parser)]
#[command(name = "nhl-player-db")]
#[command(about = "Generate NHL player database from NHL API")]
struct Cli {
    /// Output file path for the JSON database
    #[arg(short, long, default_value = "nhl_players.json")]
    output: String,
    
    /// Rate limit delay between requests in milliseconds
    #[arg(short, long, default_value = "100")]
    delay: u64,
    
    /// Start year for season data collection
    #[arg(long, default_value = "2015")]
    start_year: u32,
    
    /// End year for season data collection (current season start year)
    #[arg(long, default_value = "2025")]
    end_year: u32,
}

#[derive(Deserialize)]
struct PlayerName {
    #[serde(rename = "firstName")]
    first_name: NameField,
    #[serde(rename = "lastName")]
    last_name: NameField,
}

#[derive(Deserialize)]
struct NameField {
    #[serde(rename = "default")]
    default: String,
}

#[derive(Deserialize)]
struct RosterData {
    forwards: Option<Vec<PlayerName>>,
    defensemen: Option<Vec<PlayerName>>,
    goalies: Option<Vec<PlayerName>>,
}

#[derive(Serialize)]
struct PlayerDatabase {
    teams: HashMap<String, Vec<String>>,
    generated_at: String,
    seasons_covered: Vec<String>,
}

// List of current NHL team codes
const CURRENT_TEAM_CODES: [&str; 32] = [
    "ANA", "BOS", "BUF", "CGY", "CAR", "CHI", "COL", "CBJ", "DAL", "DET",
    "EDM", "FLA", "LAK", "MIN", "MTL", "NSH", "NJD", "NYI", "NYR", "OTT",
    "PHI", "PIT", "SJS", "SEA", "STL", "TBL", "TOR", "UTA", "VAN", "VGK",
    "WSH", "WPG"
];

// Historical team codes that need to be consolidated into current teams
const HISTORICAL_TEAM_CODES: [&str; 20] = [
    "ATL",  // Atlanta Thrashers → Winnipeg Jets
    "HFD",  // Hartford Whalers → Carolina Hurricanes  
    "QUE",  // Quebec Nordiques → Colorado Avalanche
    "MNS",  // Minnesota North Stars → Dallas Stars
    "CLR",  // Colorado Rockies → New Jersey Devils
    "KCS",  // Kansas City Scouts → New Jersey Devils (via Colorado)
    "ATF",  // Atlanta Flames → Calgary Flames
    "WPG1", // Original Winnipeg Jets → Arizona Coyotes (now Utah)
    "PHX",  // Phoenix Coyotes → Utah Hockey Club
    "ARI",  // Arizona Coyotes → Utah Hockey Club
    "MIG",  // Mighty Ducks → Anaheim Ducks
    "TBL1", // Tampa Bay Lightning (historical code if different)
    "FLA1", // Florida Panthers (historical code if different)
    "SJS1", // San Jose Sharks (historical code if different)
    "OTT1", // Ottawa Senators (historical code if different)
    "NSH1", // Nashville Predators (historical code if different)
    "CBJ1", // Columbus Blue Jackets (historical code if different)
    "MIN1", // Minnesota Wild (historical code if different)
    "VGK1", // Vegas Golden Knights (historical code if different)
    "SEA1", // Seattle Kraken (historical code if different)
];

// Team relocation mapping: historical_code -> current_code
fn get_team_mapping() -> HashMap<&'static str, &'static str> {
    let mut mapping = HashMap::new();
    
    // Major relocations and name changes
    mapping.insert("ATL", "WPG");     // Atlanta Thrashers → Winnipeg Jets (2011)
    mapping.insert("HFD", "CAR");     // Hartford Whalers → Carolina Hurricanes (1997)
    mapping.insert("QUE", "COL");     // Quebec Nordiques → Colorado Avalanche (1995)
    mapping.insert("MNS", "DAL");     // Minnesota North Stars → Dallas Stars (1993)
    mapping.insert("CLR", "NJD");     // Colorado Rockies → New Jersey Devils (1982)
    mapping.insert("KCS", "NJD");     // Kansas City Scouts → New Jersey Devils (via Colorado, 1976)
    mapping.insert("ATF", "CGY");     // Atlanta Flames → Calgary Flames (1980)
    mapping.insert("WPG1", "UTA");    // Original Winnipeg Jets → Arizona → Utah (1996)
    mapping.insert("PHX", "UTA");     // Phoenix Coyotes → Utah Hockey Club (2024)
    mapping.insert("ARI", "UTA");     // Arizona Coyotes → Utah Hockey Club (2024)
    
    // Name changes (same location)
    mapping.insert("MIG", "ANA");     // Mighty Ducks → Anaheim Ducks (2006)
    
    // Add current teams to themselves (identity mapping)
    for &team in CURRENT_TEAM_CODES.iter() {
        mapping.insert(team, team);
    }
    
    mapping
}

// Get all team codes to fetch (current + historical)
fn get_all_team_codes() -> Vec<&'static str> {
    let mut codes = CURRENT_TEAM_CODES.to_vec();
    codes.extend_from_slice(&HISTORICAL_TEAM_CODES);
    codes
}

async fn fetch_roster(client: &reqwest::Client, team_code: &str, season: &str) -> Result<RosterData, Box<dyn std::error::Error>> {
    let url = format!("https://api-web.nhle.com/v1/roster/{}/{}", team_code, season);
    
    let response = client
        .get(&url)
        .header("User-Agent", "NHL Player Database Generator 1.0")
        .send()
        .await?;
    
    if response.status().is_success() {
        let roster_data: RosterData = response.json().await?;
        Ok(roster_data)
    } else {
        Err(format!("HTTP {} for {}/{}", response.status(), team_code, season).into())
    }
}

fn extract_players(roster_data: &RosterData) -> Vec<String> {
    let mut players = Vec::new();
    
    if let Some(forwards) = &roster_data.forwards {
        for player in forwards {
            players.push(format!("{} {}", player.first_name.default, player.last_name.default));
        }
    }
    
    if let Some(defensemen) = &roster_data.defensemen {
        for player in defensemen {
            players.push(format!("{} {}", player.first_name.default, player.last_name.default));
        }
    }
    
    if let Some(goalies) = &roster_data.goalies {
        for player in goalies {
            players.push(format!("{} {}", player.first_name.default, player.last_name.default));
        }
    }
    
    players
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    
    println!("🏒 NHL Player Database Generator");
    println!("Output file: {}", cli.output);
    println!("Rate limit delay: {}ms", cli.delay);
    println!("Seasons: {}-{} to {}-{}", cli.start_year, cli.start_year + 1, cli.end_year, cli.end_year + 1);
    
    // Generate seasons
    let seasons: Vec<String> = (cli.start_year..cli.end_year)
        .map(|year| format!("{}{}", year, year + 1))
        .collect();
    
    println!("Collecting data for {} seasons and {} teams (including historical)...", seasons.len(), get_all_team_codes().len());
    
    // Create HTTP client with timeout and connection pooling
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(2)
        .build()?;
    
    let team_mapping = get_team_mapping();
    let all_team_codes = get_all_team_codes();
    let mut consolidated_database: HashMap<String, HashSet<String>> = HashMap::new();
    
    // Initialize current teams in the database
    for &current_team in CURRENT_TEAM_CODES.iter() {
        consolidated_database.insert(current_team.to_string(), HashSet::new());
    }
    
    let total_requests = all_team_codes.len() * seasons.len();
    let mut completed_requests = 0;
    
    for (team_idx, &team_code) in all_team_codes.iter().enumerate() {
        let mut team_players = HashSet::new();
        
        for (season_idx, season) in seasons.iter().enumerate() {
            match fetch_roster(&client, team_code, season).await {
                Ok(roster_data) => {
                    let players = extract_players(&roster_data);
                    for player in players {
                        team_players.insert(player);
                    }
                    if !team_players.is_empty() {
                        println!("✓ {}/{} - Found {} players", team_code, season, team_players.len());
                    }
                }
                Err(e) => {
                    // Only log errors for seasons where we expect data
//                    if season >= "19671968" { // NHL expansion era
                        eprintln!("⚠️  Failed to fetch {}/{}: {}", team_code, season, e);
//                    }
                }
            }
            
            completed_requests += 1;
            
            // Progress indicator
            if completed_requests % 20 == 0 {
                println!("Progress: {}/{} requests completed ({:.1}%)", 
                    completed_requests, total_requests, 
                    (completed_requests as f64 / total_requests as f64) * 100.0);
            }
            
            // Rate limiting - sleep between requests
            sleep(Duration::from_millis(cli.delay)).await;
        }
        
        // Consolidate players into current team
        if let Some(&current_team) = team_mapping.get(team_code) {
            if let Some(current_team_players) = consolidated_database.get_mut(current_team) {
                for player in &team_players {
                    current_team_players.insert(player.clone());
                }
            }
            
            println!("🏒 Completed {} ({}/{}) - {} unique players → consolidated into {}", 
                team_code, team_idx + 1, all_team_codes.len(), team_players.len(), current_team);
        } else {
            eprintln!("⚠️  No mapping found for team code: {}", team_code);
        }
    }
    
    // Convert HashSet to Vec for serialization and create final database structure
    let teams: HashMap<String, Vec<String>> = consolidated_database
        .into_iter()
        .map(|(team, players)| {
            let mut player_list: Vec<String> = players.into_iter().collect();
            player_list.sort(); // Sort players alphabetically
            (team, player_list)
        })
        .collect();
    
    let database = PlayerDatabase {
        teams,
        generated_at: chrono::Utc::now().to_rfc3339(),
        seasons_covered: seasons,
    };
    
    // Calculate total unique players across all teams
    let total_players: usize = database.teams.values().map(|players| players.len()).sum();
    
    println!("\n📊 Database Summary:");
    println!("   Teams: {}", database.teams.len());
    println!("   Total players: {}", total_players);
    println!("   Seasons covered: {} to {}", cli.start_year, cli.end_year);
    
    // Write to JSON file
    let json = serde_json::to_string_pretty(&database)?;
    fs::write(&cli.output, json)?;
    
    println!("✅ Database saved to: {}", cli.output);
    println!("📈 File size: {:.2} KB", fs::metadata(&cli.output)?.len() as f64 / 1024.0);
    
    Ok(())
}
