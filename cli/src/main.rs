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
    
    /// Start year for season data collection (legacy mode)
    #[arg(long, default_value = "2015")]
    start_year: u32,
    
    /// End year for season data collection (legacy mode)
    #[arg(long, default_value = "2025")]
    end_year: u32,
    
    /// Include game-by-game data to find missing players (legacy mode)
    #[arg(long, default_value = "false")]
    include_games: bool,
    
    /// Use new player search API instead of legacy team/season iteration
    #[arg(long, default_value = "true")]
    use_player_search: bool,
}

#[derive(Deserialize)]
struct PlayerName {
    #[serde(rename = "firstName")]
    #[serde(deserialize_with = "deserialize_name_field")]
    first_name: String,
    #[serde(rename = "lastName")]
    #[serde(deserialize_with = "deserialize_name_field")]
    last_name: String,
}

// Custom deserializer to extract the "default" field
fn deserialize_name_field<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let name_field = NameField::deserialize(deserializer)?;
    Ok(name_field.default)
}

// Custom deserializer for optional name fields
fn deserialize_optional_name_field<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt_name_field = Option::<NameField>::deserialize(deserializer)?;
    Ok(opt_name_field.map(|field| field.default))
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

// Game data structures for extracting players from game logs
#[derive(Deserialize)]
struct ScheduleResponse {
    games: Vec<GameInfo>,
}

#[derive(Deserialize)]
struct GameInfo {
    id: u64,
    #[serde(rename = "awayTeam")]
    away_team: TeamGameInfo,
    #[serde(rename = "homeTeam")]
    home_team: TeamGameInfo,
}

#[derive(Deserialize)]
struct TeamGameInfo {
    abbrev: String,
}

#[derive(Deserialize)]
struct GameDetails {
    #[serde(rename = "awayTeam")]
    away_team: Option<TeamDetails>,
    #[serde(rename = "homeTeam")]
    home_team: Option<TeamDetails>,
}

#[derive(Deserialize)]
struct TeamDetails {
    #[serde(rename = "skaters")]
    skaters: Option<Vec<GamePlayer>>,
    #[serde(rename = "goalies")]
    goalies: Option<Vec<GamePlayer>>,
}

#[derive(Deserialize)]
struct GamePlayer {
    #[serde(rename = "firstName")]
    #[serde(deserialize_with = "deserialize_name_field")]
    first_name: String,
    #[serde(rename = "lastName")]
    #[serde(deserialize_with = "deserialize_name_field")]
    last_name: String,
}

// New data structures for player search API
#[derive(Deserialize)]
struct PlayerSearchResult {
    #[serde(rename = "playerId")]
    player_id: String,
    name: String,
    #[serde(rename = "positionCode")]
    position_code: String,
    #[serde(rename = "lastSeasonId")]
    last_season_id: Option<String>,
    active: bool,
}

#[derive(Deserialize)]
struct PlayerDetails {
    #[serde(rename = "playerId")]
    player_id: u64,
    #[serde(rename = "firstName")]
    #[serde(deserialize_with = "deserialize_name_field")]
    first_name: String,
    #[serde(rename = "lastName")]
    #[serde(deserialize_with = "deserialize_name_field")]
    last_name: String,
    #[serde(rename = "birthDate")]
    birth_date: Option<String>,
    #[serde(rename = "birthCity")]
    #[serde(deserialize_with = "deserialize_optional_name_field")]
    birth_city: Option<String>,
    #[serde(rename = "birthCountry")]
    birth_country: Option<String>,
    position: Option<String>,
    #[serde(rename = "heightInInches")]
    height_in_inches: Option<u32>,
    #[serde(rename = "weightInPounds")]
    weight_in_pounds: Option<u32>,
    #[serde(rename = "currentTeamAbbrev")]
    current_team_abbrev: Option<String>,
    #[serde(rename = "seasonTotals")]
    season_totals: Option<Vec<SeasonTotal>>,
}

#[derive(Deserialize)]
struct SeasonTotal {
    season: u32,
    #[serde(rename = "teamName")]
    #[serde(deserialize_with = "deserialize_optional_name_field")]
    team_name: Option<String>,
}

#[derive(Serialize, Clone, Hash, PartialEq, Eq)]
struct PlayerInfo {
    id: String,
    name: String,
    birth_date: Option<String>,
    birth_place: Option<String>, // "City, Country" format
    position: Option<String>,
}

#[derive(Serialize)]
struct PlayerDatabase {
    teams: HashMap<String, Vec<PlayerInfo>>,
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
const HISTORICAL_TEAM_CODES: [&str; 11] = [
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

async fn fetch_all_players(client: &reqwest::Client) -> Result<Vec<PlayerSearchResult>, Box<dyn std::error::Error>> {
    let url = "https://search.d3.nhle.com/api/v1/search/player?culture=en-us&limit=24000&q=*";
    
    println!("🔍 Fetching all players from NHL search API...");
    
    let response = client
        .get(url)
        .header("User-Agent", "NHL Player Database Generator 1.0")
        .send()
        .await?;
    
    if response.status().is_success() {
        let players: Vec<PlayerSearchResult> = response.json().await?;
        println!("✅ Found {} players in search results", players.len());
        Ok(players)
    } else {
        Err(format!("HTTP {} for player search", response.status()).into())
    }
}

async fn fetch_player_details(client: &reqwest::Client, player_id: &str) -> Result<PlayerDetails, Box<dyn std::error::Error>> {
    let url = format!("https://api-web.nhle.com/v1/player/{player_id}/landing");
    
    println!("   Fetching details for player ID: {}", player_id);
    let response = client
        .get(&url)
        .header("User-Agent", "NHL Player Database Generator 1.0")
        .send()
        .await?;
    
    if response.status().is_success() {
        let player_details: PlayerDetails = response.json().await?;
        Ok(player_details)
    } else {
        Err(format!("HTTP {} for player {}", response.status(), player_id).into())
    }
}

async fn build_database_from_player_search(
    client: &reqwest::Client,
    delay_ms: u64,
    start_year: u32,
    end_year: u32,
) -> Result<HashMap<String, HashSet<PlayerInfo>>, Box<dyn std::error::Error>> {
    let all_players = fetch_all_players(client).await?;
    let team_mapping = get_team_mapping();
    let mut consolidated_database: HashMap<String, HashSet<PlayerInfo>> = HashMap::new();
    
    let team_codes: HashMap<&str, &str> = [
        ("Anaheim Ducks", "ANA"), ("Boston Bruins", "BOS"), ("Buffalo Sabres", "BUF"),
        ("Calgary Flames", "CGY"), ("Carolina Hurricanes", "CAR"), ("Chicago Blackhawks", "CHI"),
        ("Colorado Avalanche", "COL"), ("Columbus Blue Jackets", "CBJ"), ("Dallas Stars", "DAL"),
        ("Detroit Red Wings", "DET"), ("Edmonton Oilers", "EDM"), ("Florida Panthers", "FLA"),
        ("Los Angeles Kings", "LAK"), ("Minnesota Wild", "MIN"), ("Montréal Canadiens", "MTL"),
        ("Nashville Predators", "NSH"), ("New Jersey Devils", "NJD"), ("New York Islanders", "NYI"),
        ("New York Rangers", "NYR"), ("Ottawa Senators", "OTT"), ("Philadelphia Flyers", "PHI"),
        ("Pittsburgh Penguins", "PIT"), ("San Jose Sharks", "SJS"), ("Seattle Kraken", "SEA"),
        ("St. Louis Blues", "STL"), ("Tampa Bay Lightning", "TBL"), ("Toronto Maple Leafs", "TOR"),
        ("Utah Hockey Club", "UTA"), ("Vancouver Canucks", "VAN"), ("Vegas Golden Knights", "VGK"),
        ("Washington Capitals", "WSH"), ("Winnipeg Jets", "WPG"),
        // Historical team names that map to team codes
        ("Atlanta Thrashers", "ATL"), ("Hartford Whalers", "HFD"), ("Quebec Nordiques", "QUE"),
        ("Minnesota North Stars", "MNS"), ("Colorado Rockies", "CLR"), ("Kansas City Scouts", "KCS"),
        ("Atlanta Flames", "ATF"), ("Phoenix Coyotes", "PHX"), ("Arizona Coyotes", "ARI"),
        ("Mighty Ducks of Anaheim", "MIG"), ("Winnipeg Jets (1979)", "WPG1"),
    ].iter().cloned().collect();

    // Initialize current teams in the database
    for &current_team in CURRENT_TEAM_CODES.iter() {
        consolidated_database.insert(current_team.to_string(), HashSet::new());
    }
    
    println!("🏒 Processing {} players to build team associations...", all_players.len());
    println!("📅 Including seasons {}-{} to {}-{}", start_year, start_year + 1, end_year, end_year + 1);
    
    let total_players = all_players.len();
    let mut processed_count = 0;
    let mut players_with_teams = 0;
    let mut api_errors = 0;
    
    for player in &all_players {
        sleep(Duration::from_millis(delay_ms)).await;
        processed_count += 1;
        
        // Progress indicator every 100 players
        if processed_count % 100 == 0 {
            println!("📊 Progress: {}/{} players processed ({:.1}%), {} with teams, {} errors", 
                     processed_count, total_players, 
                     (processed_count as f64 / total_players as f64) * 100.0,
                     players_with_teams, api_errors);
        }
        
        match fetch_player_details(client, &player.player_id).await {
            Ok(details) => {
                let full_name = format!("{} {}", details.first_name, details.last_name);
                let mut player_teams = HashSet::new();
                println!("   Processing player: {} (ID: {})", full_name, player.player_id);
                
                // Create PlayerInfo object
                let birth_place = match (&details.birth_city, &details.birth_country) {
                    (Some(city), Some(country)) => Some(format!("{}, {}", city, country)),
                    (None, Some(country)) => Some(country.clone()),
                    _ => None,
                };
                
                let player_info = PlayerInfo {
                    id: details.player_id.to_string(),
                    name: full_name.clone(),
                    birth_date: details.birth_date.clone(),
                    birth_place,
                    position: details.position.clone(),
                };
                
                // Extract teams from season totals
                if let Some(season_totals) = &details.season_totals {
                    for season_total in season_totals {
                        // Filter by season range (convert season format)
                        let season_start_year = season_total.season / 10000;
                        if season_start_year >= start_year && season_start_year <= end_year {
                            if let Some(team_full_name) = &season_total.team_name {
                                if let Some(&team_code) = team_codes.get(team_full_name.as_str()) {
                                    player_teams.insert(team_code.to_string());
                                }
                            }
                        }
                    }
                }
                
                // Map teams to current teams and add player
                if !player_teams.is_empty() {
                    players_with_teams += 1;
                    for team_code in &player_teams {
                        if let Some(&current_team) = team_mapping.get(team_code.as_str()) {
                            if let Some(team_players) = consolidated_database.get_mut(current_team) {
                                team_players.insert(player_info.clone());
                            }
                        }
                    }
                }
            }
            Err(e) => {
                println!("⚠️  Failed to fetch player details (ID: {}): {}", player.player_id, e);
                api_errors += 1;
            }
        }
    }
    
    println!("✅ Completed processing {} players", processed_count);
    println!("   Players with team data: {}", players_with_teams);
    println!("   API errors: {}", api_errors);
    
    Ok(consolidated_database)
}

async fn build_database_legacy(
    client: &reqwest::Client,
    seasons: &[String],
    delay_ms: u64,
    include_games: bool,
) -> Result<HashMap<String, HashSet<String>>, Box<dyn std::error::Error>> {
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
        let mut roster_players = HashSet::new();
        let mut game_players = HashSet::new();
        
        for (_season_idx, season) in seasons.iter().enumerate() {
            // Fetch roster data
            match fetch_roster(client, team_code, season).await {
                Ok(roster_data) => {
                    let players = extract_players(&roster_data);
                    for player in players {
                        roster_players.insert(player.clone());
                        team_players.insert(player);
                    }
                    if !roster_players.is_empty() {
                        println!("✓ {}/{} - Roster: {} players", team_code, season, roster_players.len());
                    }
                }
                Err(e) => {
                    eprintln!("⚠️  Failed to fetch roster {}/{}: {}", team_code, season, e);
                }
            }
            
            // Fetch game data if enabled
            if include_games {
                match fetch_players_from_games(client, team_code, season, delay_ms).await {
                    Ok(season_game_players) => {
                        let mut new_players = 0;
                        for player in &season_game_players {
                            if !roster_players.contains(player) {
                                new_players += 1;
                                game_players.insert(player.clone());
                                team_players.insert(player.clone());
                            }
                        }
                        if new_players > 0 {
                            println!("  📋 {}/{} - Games: {} additional players not in roster", team_code, season, new_players);
                        }
                    }
                    Err(e) => {
                        eprintln!("⚠️  Failed to fetch game data {}/{}: {}", team_code, season, e);
                    }
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
            sleep(Duration::from_millis(delay_ms)).await;
        }
        
        // Consolidate players into current team
        if let Some(&current_team) = team_mapping.get(team_code) {
            if let Some(current_team_players) = consolidated_database.get_mut(current_team) {
                for player in &team_players {
                    current_team_players.insert(player.clone());
                }
            }
            
            let roster_count = roster_players.len();
            let game_count = game_players.len();
            let total_count = team_players.len();
            
            if include_games && game_count > 0 {
                println!("🏒 Completed {} ({}/{}) - {} total players ({} roster + {} from games) → consolidated into {}", 
                    team_code, team_idx + 1, all_team_codes.len(), total_count, roster_count, game_count, current_team);
            } else {
                println!("🏒 Completed {} ({}/{}) - {} unique players → consolidated into {}", 
                    team_code, team_idx + 1, all_team_codes.len(), total_count, current_team);
            }
        } else {
            eprintln!("⚠️  No mapping found for team code: {}", team_code);
        }
    }
    
    Ok(consolidated_database)
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

async fn fetch_team_schedule(client: &reqwest::Client, team_code: &str, season: &str) -> Result<ScheduleResponse, Box<dyn std::error::Error>> {
    // Try different API endpoint formats
    let urls = vec![
        format!("https://api-web.nhle.com/v1/club-schedule-season/{}/{}", team_code, season),
        format!("https://api-web.nhle.com/v1/schedule/{}/{}", team_code, season),
        format!("https://statsapi.web.nhl.com/api/v1/teams/{}/schedule?season={}", team_code, season),
    ];
    
    for url in urls {
        let response = client
            .get(&url)
            .header("User-Agent", "NHL Player Database Generator 1.0")
            .send()
            .await?;
        
        if response.status().is_success() {
            match response.json::<ScheduleResponse>().await {
                Ok(schedule_data) => return Ok(schedule_data),
                Err(_) => continue, // Try next URL format
            }
        }
    }
    
    Err(format!("All schedule API endpoints failed for {}/{}", team_code, season).into())
}

async fn fetch_game_details(client: &reqwest::Client, game_id: u64) -> Result<GameDetails, Box<dyn std::error::Error>> {
    let url = format!("https://api-web.nhle.com/v1/gamecenter/{}/boxscore", game_id);
    
    let response = client
        .get(&url)
        .header("User-Agent", "NHL Player Database Generator 1.0")
        .send()
        .await?;
    
    if response.status().is_success() {
        let game_data: GameDetails = response.json().await?;
        Ok(game_data)
    } else {
        Err(format!("HTTP {} for game {}", response.status(), game_id).into())
    }
}

async fn fetch_players_from_games(
    client: &reqwest::Client, 
    team_code: &str, 
    season: &str,
    delay_ms: u64
) -> Result<HashSet<String>, Box<dyn std::error::Error>> {
    let mut game_players = HashSet::new();
    
    // Fetch team schedule for the season
    match fetch_team_schedule(client, team_code, season).await {
        Ok(schedule) => {
            println!("    📅 Found {} games for {}/{}", schedule.games.len(), team_code, season);
            
            // Limit to first 10 games for now to avoid too many requests
            let games_to_check = schedule.games.iter().take(10);
            
            for game in games_to_check {
                // Check if this team was playing in this game
                if game.away_team.abbrev == team_code || game.home_team.abbrev == team_code {
                    sleep(Duration::from_millis(delay_ms)).await;
                    
                    match fetch_game_details(client, game.id).await {
                        Ok(game_details) => {
                            // Extract players from the team we're interested in
                            let team_details = if game.away_team.abbrev == team_code {
                                &game_details.away_team
                            } else {
                                &game_details.home_team
                            };
                            
                            if let Some(team_data) = team_details {
                                // Extract skaters
                                if let Some(skaters) = &team_data.skaters {
                                    for player in skaters {
                                        let full_name = format!("{} {}", player.first_name, player.last_name);
                                        game_players.insert(full_name);
                                    }
                                }
                                
                                // Extract goalies
                                if let Some(goalies) = &team_data.goalies {
                                    for player in goalies {
                                        let full_name = format!("{} {}", player.first_name, player.last_name);
                                        game_players.insert(full_name);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("    ⚠️  Failed to fetch game {}: {}", game.id, e);
                        }
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("    ⚠️  Failed to fetch schedule for {}/{}: {}", team_code, season, e);
        }
    }
    
    Ok(game_players)
}

fn extract_players(roster_data: &RosterData) -> Vec<String> {
    let mut players = Vec::new();
    
    if let Some(forwards) = &roster_data.forwards {
        for player in forwards {
            players.push(format!("{} {}", player.first_name, player.last_name));
        }
    }
    
    if let Some(defensemen) = &roster_data.defensemen {
        for player in defensemen {
            players.push(format!("{} {}", player.first_name, player.last_name));
        }
    }
    
    if let Some(goalies) = &roster_data.goalies {
        for player in goalies {
            players.push(format!("{} {}", player.first_name, player.last_name));
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
    
    // Create HTTP client with timeout and connection pooling
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(2)
        .build()?;
    
    let consolidated_database = if cli.use_player_search {
        println!("🚀 Using new player search API approach");
        println!("📅 Season filter: {}-{} to {}-{}", cli.start_year, cli.start_year + 1, cli.end_year, cli.end_year + 1);
        
        build_database_from_player_search(&client, cli.delay, cli.start_year, cli.end_year).await?
    } else {
        // TODO: Update legacy function to return PlayerInfo format
        println!("❌ Legacy approach temporarily disabled. Please use --use-player-search flag.");
        std::process::exit(1);
    };
    
    // Generate seasons list for database metadata
    let seasons: Vec<String> = (cli.start_year..=cli.end_year)
        .map(|year| format!("{}{}", year, year + 1))
        .collect();

    // Convert HashSet to Vec for serialization and create final database structure
    let teams: HashMap<String, Vec<PlayerInfo>> = consolidated_database
        .into_iter()
        .map(|(team, players)| {
            let mut player_list: Vec<PlayerInfo> = players.into_iter().collect();
            player_list.sort_by(|a, b| a.name.cmp(&b.name)); // Sort players alphabetically by name
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
    
    if cli.use_player_search {
        println!("   Data source: NHL Player Search API (comprehensive)");
        println!("   Method: Individual player career analysis");
    } else if cli.include_games {
        println!("   Data sources: Team rosters + game-by-game player appearances");
        println!("   Note: Game data limited to first 10 games per team/season for API efficiency");
    } else {
        println!("   Data sources: Team rosters only");
    }
    
    // Write to JSON file
    let json = serde_json::to_string_pretty(&database)?;
    fs::write(&cli.output, json)?;
    
    println!("✅ Database saved to: {}", cli.output);
    println!("📈 File size: {:.2} KB", fs::metadata(&cli.output)?.len() as f64 / 1024.0);
    
    Ok(())
}
