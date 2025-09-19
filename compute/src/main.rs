//! Default Compute template program.

use fastly::http::{header, Method, StatusCode};
use fastly::kv_store;
use fastly::{mime, Error, Request, Response};
use std::collections::HashMap;

fn get(version: u32) -> Result<serde_json::Value, Error> {
    let store = kv_store::KVStore::open("journeyman")
        .expect("failed to open KV store")
        .unwrap();
    let mut res = if version == 1 {
        store.lookup("players")
    } else if version == 2 {
        store.lookup("playersv2")
    } else {
        unimplemented!()
        // store.lookup("unimplemented")
    }?;
    let body = res.take_body();
    let json: serde_json::Value =
        serde_json::from_str(&body.into_string()).expect("json deserialization failed");
    Ok(json)
}

// Generate deterministic daily teams based on current date
fn get_daily_teams() -> Result<serde_json::Value, Error> {
    // Get current date as days since epoch for deterministic seed
    let now = std::time::SystemTime::now();
    let epoch = std::time::UNIX_EPOCH;
    let duration = now.duration_since(epoch).unwrap();
    let days_since_epoch = duration.as_secs() / (24 * 60 * 60);
    
    // All team names
    let all_teams = vec![
        "Anaheim Ducks", "Boston Bruins", "Buffalo Sabres", "Calgary Flames",
        "Carolina Hurricanes", "Chicago Blackhawks", "Colorado Avalanche", 
        "Columbus Blue Jackets", "Dallas Stars", "Detroit Red Wings",
        "Edmonton Oilers", "Florida Panthers", "Los Angeles Kings", 
        "Minnesota Wild", "Montreal Canadiens", "Nashville Predators",
        "New Jersey Devils", "New York Islanders", "New York Rangers", 
        "Ottawa Senators", "Philadelphia Flyers", "Pittsburgh Penguins",
        "San Jose Sharks", "Seattle Kraken", "St. Louis Blues", 
        "Tampa Bay Lightning", "Toronto Maple Leafs", "Utah Hockey Club",
        "Vancouver Canucks", "Vegas Golden Knights", "Washington Capitals", 
        "Winnipeg Jets"
    ];
    
    // Simple deterministic selection using day as seed
    let mut selected_teams = Vec::new();
    let mut seed = days_since_epoch as usize;
    
    // Select 8 teams deterministically
    let mut available_teams = all_teams.clone();
    for _ in 0..8 {
        let index = seed % available_teams.len();
        selected_teams.push(available_teams.remove(index));
        seed = (seed * 1103515245 + 12345) % (1 << 31); // Simple LCG
    }
    
    let response = serde_json::json!({
        "teams": selected_teams,
        "date": format!("{}", days_since_epoch),
        "generated_at": format!("{:?}", now)
    });
    
    Ok(response)
}

// Calculate rarity score based on actual player usage in daily submissions
fn calculate_rarity_score(players: &[serde_json::Value], game_teams: &[String]) -> Result<serde_json::Value, Error> {
    // Get the full player database to calculate team specialization
    let player_data = get(2)?;
    
    // Team code mapping
    let team_codes: HashMap<&str, &str> = [
        ("Anaheim Ducks", "ANA"), ("Boston Bruins", "BOS"), ("Buffalo Sabres", "BUF"),
        ("Calgary Flames", "CGY"), ("Carolina Hurricanes", "CAR"), ("Chicago Blackhawks", "CHI"),
        ("Colorado Avalanche", "COL"), ("Columbus Blue Jackets", "CBJ"), ("Dallas Stars", "DAL"),
        ("Detroit Red Wings", "DET"), ("Edmonton Oilers", "EDM"), ("Florida Panthers", "FLA"),
        ("Los Angeles Kings", "LAK"), ("Minnesota Wild", "MIN"), ("Montreal Canadiens", "MTL"),
        ("Nashville Predators", "NSH"), ("New Jersey Devils", "NJD"), ("New York Islanders", "NYI"),
        ("New York Rangers", "NYR"), ("Ottawa Senators", "OTT"), ("Philadelphia Flyers", "PHI"),
        ("Pittsburgh Penguins", "PIT"), ("San Jose Sharks", "SJS"), ("Seattle Kraken", "SEA"),
        ("St. Louis Blues", "STL"), ("Tampa Bay Lightning", "TBL"), ("Toronto Maple Leafs", "TOR"),
        ("Utah Hockey Club", "UTA"), ("Vancouver Canucks", "VAN"), ("Vegas Golden Knights", "VGK"),
        ("Washington Capitals", "WSH"), ("Winnipeg Jets", "WPG")
    ].iter().cloned().collect();
    
    let mut total_rarity_score = 0.0;
    let mut player_scores = Vec::new();
    
    for player_obj in players {
        // Extract player name and ID from the player object
        let player_name = if let Some(name) = player_obj.get("name").and_then(|n| n.as_str()) {
            name
        } else if let Some(name) = player_obj.as_str() {
            // Fallback for string format
            name
        } else {
            continue; // Skip invalid player objects
        };
        
        let player_id = player_obj.get("id").and_then(|id| id.as_str());
        
        // Find how many total teams this player played for
        let mut total_teams_played = 0;
        let mut teams_in_current_game = 0;
        let mut player_info = None;
        
        if let Some(teams_obj) = player_data["teams"].as_object() {
            for (team_code, team_players) in teams_obj {
                if let Some(players_array) = team_players.as_array() {
                    // Look for player in the new PlayerInfo format
                    let team_has_player = players_array.iter()
                        .any(|p| {
                            if let Some(db_player_obj) = p.as_object() {
                                // Try to match by ID first (most reliable), then fall back to name
                                if let (Some(player_id), Some(db_id)) = (player_id, db_player_obj.get("id").and_then(|id| id.as_str())) {
                                    let matches = player_id == db_id;
                                    if matches && player_info.is_none() {
                                        player_info = Some(p.clone());
                                    }
                                    matches
                                } else if let Some(db_player_name) = db_player_obj.get("name").and_then(|n| n.as_str()) {
                                    let matches = db_player_name.eq_ignore_ascii_case(player_name);
                                    if matches && player_info.is_none() {
                                        player_info = Some(p.clone());
                                    }
                                    matches
                                } else {
                                    false
                                }
                            } else {
                                // Fallback for old string format
                                p.as_str().unwrap_or("").eq_ignore_ascii_case(player_name)
                            }
                        });
                    
                    if team_has_player {
                        total_teams_played += 1;
                        
                        // Check if this team is in the current game
                        let team_name = team_codes.iter()
                            .find(|(_, code)| *code == team_code)
                            .map(|(name, _)| *name);
                        
                        if let Some(name) = team_name {
                            if game_teams.contains(&name.to_string()) {
                                teams_in_current_game += 1;
                            }
                        }
                    }
                }
            }
        }
        
        // Calculate specialization score: higher score for players who played for more teams
        // in the current game, regardless of how many other teams they played for
        let specialization_ratio = if total_teams_played > 0 {
            teams_in_current_game as f64 / total_teams_played as f64
        } else {
            0.0
        };
        
        // Rarity score: reward players who contributed to more teams in the current game
        // The specialization ratio ensures we still prefer players who didn't play everywhere
        let player_rarity = teams_in_current_game as f64 * specialization_ratio;
        
        total_rarity_score += player_rarity;
        
        let mut player_score = serde_json::json!({
            "name": player_name,
            "id": player_id,
            "total_teams_played": total_teams_played,
            "teams_in_current_game": teams_in_current_game,
            "specialization_ratio": specialization_ratio,
            "rarity_score": player_rarity
        });
        
        // Include player info if found
        if let Some(info) = player_info {
            player_score["player_info"] = info;
        }
        
        player_scores.push(player_score);
    }
    
    Ok(serde_json::json!({
        "total_rarity_score": total_rarity_score,
        "player_count": players.len(),
        "average_rarity": if players.len() > 0 { total_rarity_score / players.len() as f64 } else { 0.0 },
        "players": player_scores
    }))
}

// Submit a daily solution and update usage statistics
fn submit_daily_solution(players: Vec<String>, date: String, user_id: String) -> Result<serde_json::Value, Error> {
    let store = kv_store::KVStore::open("journeyman")
        .expect("failed to open KV store")
        .unwrap();
    
    // Check if user already submitted for this date
    let submission_key = format!("daily_submission_{}_{}", date, user_id);
    if store.lookup(&submission_key).is_ok() {
        return Ok(serde_json::json!({
            "error": "already_submitted",
            "message": "You have already submitted a solution for today"
        }));
    }
    
    // Get daily teams for this date
    let daily_teams_data = get_daily_teams()?;
    let daily_teams: Vec<String> = daily_teams_data["teams"].as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|v| v.as_str().unwrap_or("").to_string())
        .collect();
    
    // Calculate current rarity score
    let player_objects: Vec<serde_json::Value> = players.iter()
        .map(|name| serde_json::json!({"name": name, "id": null}))
        .collect();
    let rarity_data = calculate_rarity_score(&player_objects, &daily_teams)?;
    
    // Update player usage statistics
    let usage_key = format!("daily_usage_{}", date);
    let mut usage_stats: HashMap<String, u32> = match store.lookup(&usage_key) {
        Ok(mut res) => {
            let body = res.take_body();
            serde_json::from_str(&body.into_string()).unwrap_or_else(|_| HashMap::new())
        },
        Err(_) => HashMap::new()
    };
    
    // Increment usage count for each player
    for player in &players {
        *usage_stats.entry(player.clone()).or_insert(0) += 1;
    }
    
    // Save updated usage statistics
    let usage_json = serde_json::to_string(&usage_stats).unwrap();
    store.insert(&usage_key, usage_json.as_bytes())?;
    
    // Save user's submission
    let submission_data = serde_json::json!({
        "players": players,
        "player_count": players.len(),
        "rarity_score": rarity_data["total_rarity_score"],
        "submitted_at": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    });
    let submission_json = serde_json::to_string(&submission_data).unwrap();
    store.insert(&submission_key, submission_json.as_bytes())?;
    
    // Get leaderboard position
    let leaderboard = get_daily_leaderboard(&date)?;
    
    Ok(serde_json::json!({
        "success": true,
        "rarity_data": rarity_data,
        "leaderboard_position": calculate_leaderboard_position(&submission_data, &leaderboard),
        "total_submissions": leaderboard["submissions"].as_array().unwrap_or(&vec![]).len()
    }))
}

// Get daily leaderboard
fn get_daily_leaderboard(date: &str) -> Result<serde_json::Value, Error> {
    let _store = kv_store::KVStore::open("journeyman")
        .expect("failed to open KV store")
        .unwrap();
    
    // This would typically scan all submissions for the date
    // For now, return a placeholder structure
    Ok(serde_json::json!({
        "date": date,
        "submissions": []
    }))
}

// Calculate where this submission ranks
fn calculate_leaderboard_position(_submission: &serde_json::Value, _leaderboard: &serde_json::Value) -> u32 {
    // Placeholder - would compare player_count first, then rarity_score
    1
}

/// The entry point for your application.
///
/// This function is triggered when your service receives a client request. It could be used to
/// route based on the request properties (such as method or path), send the request to a backend,
/// make completely new requests, and/or generate synthetic responses.
///
/// If `main` returns an error, a 500 error response will be delivered to the client.
#[fastly::main]
fn main(req: Request) -> Result<Response, Error> {
    // Log service version
    println!(
        "FASTLY_SERVICE_VERSION: {}",
        std::env::var("FASTLY_SERVICE_VERSION").unwrap_or_else(|_| String::new())
    );

	if req.get_method() == Method::OPTIONS {
        return Ok(Response::from_status(StatusCode::OK)
			.with_header("Access-Control-Allow-Origin","*")
			.with_header("Access-Control-Allow-Headers","*")
			.with_header("Vary","Origin")
            .with_body_text_plain(""))
	}    

    // Filter request methods...
    match req.get_method() {
        // Block requests with unexpected methods (but allow POST for rarity calculation)
        &Method::PUT | &Method::PATCH | &Method::DELETE => {
            return Ok(Response::from_status(StatusCode::METHOD_NOT_ALLOWED)
                .with_header(header::ALLOW, "GET, HEAD, POST, PURGE")
                .with_body_text_plain("This method is not allowed\n"))
        }

        // Let any other requests through
        _ => (),
    };

    // Pattern match on the path...
    match req.get_path() {
        "/get_players" => {
            let db = get(1)?;
            // Example of returning a JSON response.
            Ok(Response::from_status(StatusCode::OK)
                .with_content_type(mime::APPLICATION_JSON)
                .with_header("Access-Control-Allow-Origin", "*")
                .with_body(serde_json::to_string(&db).expect("failed to serialize DB")))
        },
        "/get_playersv2" => {
            let db = get(2)?;
            // Example of returning a JSON response.
            Ok(Response::from_status(StatusCode::OK)
                .with_content_type(mime::APPLICATION_JSON)
                .with_header("Access-Control-Allow-Origin", "*")
                .with_body(serde_json::to_string(&db).expect("failed to serialize DB")))
        },
        "/get_daily_teams" => {
            let daily_teams = get_daily_teams()?;
            Ok(Response::from_status(StatusCode::OK)
                .with_content_type(mime::APPLICATION_JSON)
                .with_header("Access-Control-Allow-Origin", "*")
                .with_body(serde_json::to_string(&daily_teams).expect("failed to serialize daily teams")))
        },
        "/calculate_rarity" => {
            // Parse POST body for player objects and teams
            let body = req.into_body_str();
            let request_data: serde_json::Value = match serde_json::from_str(&body) {
                Ok(p) => p,
                Err(_) => {
                    return Ok(Response::from_status(StatusCode::BAD_REQUEST)
                        .with_header("Access-Control-Allow-Origin", "*")
                        .with_body_text_plain("Invalid JSON format"))
                }
            };
            
            let players = request_data["players"].as_array()
                .ok_or_else(|| Error::msg("Missing players array"))?
                .iter()
                .map(|p| p.clone())
                .collect::<Vec<serde_json::Value>>();
                
            let teams = request_data["teams"].as_array()
                .ok_or_else(|| Error::msg("Missing teams array"))?
                .iter()
                .filter_map(|t| t.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>();

            let rarity_data = calculate_rarity_score(&players, &teams)?;
            Ok(Response::from_status(StatusCode::OK)
                .with_content_type(mime::APPLICATION_JSON)
                .with_header("Access-Control-Allow-Origin", "*")
                .with_body(serde_json::to_string(&rarity_data).expect("failed to serialize rarity data")))
        },
        "/submit_daily" => {
            // Parse POST body for submission
            let body = req.into_body_str();
            let request_data: serde_json::Value = match serde_json::from_str(&body) {
                Ok(p) => p,
                Err(_) => {
                    return Ok(Response::from_status(StatusCode::BAD_REQUEST)
                        .with_header("Access-Control-Allow-Origin", "*")
                        .with_body_text_plain("Invalid JSON format"))
                }
            };
            
            let players = request_data["players"].as_array()
                .ok_or_else(|| Error::msg("Missing players array"))?
                .iter()
                .filter_map(|p| p.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>();
                
            let date = request_data["date"].as_str()
                .ok_or_else(|| Error::msg("Missing date"))?
                .to_string();
                
            let user_id = request_data["user_id"].as_str()
                .ok_or_else(|| Error::msg("Missing user_id"))?
                .to_string();
            
            let result = submit_daily_solution(players, date, user_id)?;
            Ok(Response::from_status(StatusCode::OK)
                .with_content_type(mime::APPLICATION_JSON)
                .with_header("Access-Control-Allow-Origin", "*")
                .with_body(serde_json::to_string(&result).expect("failed to serialize submission result")))
        },

        // Catch all other requests and return a 404.
        _ => Ok(Response::from_status(StatusCode::NOT_FOUND)
            .with_header("Access-Control-Allow-Origin", "*")
            .with_body_text_plain("The page you requested could not be found\n")),
    }
}
