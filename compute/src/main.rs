//! Default Compute template program.

use fastly::http::header::LAST_MODIFIED;
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

fn get_teams_played_for(player_id: &str) -> Result<Vec<String>, Error> {
    let player_data = get(2)?;
    let mut teams = Vec::new();
    
    if let Some(teams_obj) = player_data["teams"].as_object() {
        for (team_code, team_players) in teams_obj {
            if let Some(players_array) = team_players.as_array() {
                // Look for player in the new PlayerInfo format
                let team_has_player = players_array.iter()
                    .any(|p| {
                        if let Some(db_player_obj) = p.as_object() {
                            // Try to match by ID first (most reliable), then fall back to name
                            if let Some(db_id) = db_player_obj.get("id").and_then(|id| id.as_str()) {
                                player_id == db_id
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    });
                
                if team_has_player {
                    teams.push(team_code.to_string());
                }
            }
        }
    }
  
    Ok(teams)
}

// Calculate overlap score based on actual player usage in daily submissions
fn calculate_overlap_score(players: &[serde_json::Value], game_teams: &[String]) -> Result<serde_json::Value, Error> {
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
    
    let mut total_overlap_score = 0.0;
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
        
        // overlap score: reward players who contributed to more teams in the current game
        // The specialization ratio ensures we still prefer players who didn't play everywhere
        let player_overlap_score = teams_in_current_game as f64 * specialization_ratio;

        total_overlap_score += player_overlap_score;

        let mut player_score = serde_json::json!({
            "name": player_name,
            "id": player_id,
            "total_teams_played": total_teams_played,
            "teams_in_current_game": teams_in_current_game,
            "specialization_ratio": specialization_ratio,
            "overlap_score": player_overlap_score
        });
        
        // Include player info if found
        if let Some(info) = player_info {
            player_score["player_info"] = info;
        }
        
        player_scores.push(player_score);
    }
    
    Ok(serde_json::json!({
        "total_overlap_score": total_overlap_score,
        "player_count": players.len(),
        "average_overlap": if players.len() > 0 { total_overlap_score / players.len() as f64 } else { 0.0 },
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
    
    // Calculate current overlap score
    let player_objects: Vec<serde_json::Value> = players.iter()
        .map(|name| serde_json::json!({"name": name, "id": null}))
        .collect();
    let overlap_data = calculate_overlap_score(&player_objects, &daily_teams)?;
    
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
        "overlap_score": overlap_data["total_overlap_score"],
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
        "overlap_data": overlap_data,
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
    // Placeholder - would compare player_count first, then overlap_score
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
        // Block requests with unexpected methods (but allow POST for overlap calculation)
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
        "/calculate_overlap" => {
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

            let overlap_data = calculate_overlap_score(&players, &teams)?;
            Ok(Response::from_status(StatusCode::OK)
                .with_content_type(mime::APPLICATION_JSON)
                .with_header("Access-Control-Allow-Origin", "*")
                .with_body(serde_json::to_string(&overlap_data).expect("failed to serialize overlap data")))
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

        "/get_hint" => {
            // Parse POST body for teams and used_players
            let body = req.into_body_str();
            let request_data: serde_json::Value = match serde_json::from_str(&body) {
                Ok(p) => p,
                Err(_) => {
                    return Ok(Response::from_status(StatusCode::BAD_REQUEST)
                        .with_header("Access-Control-Allow-Origin", "*")
                        .with_body_text_plain("Invalid JSON format"))
                }
            };

            let teams = request_data["teams"].as_array()
                .ok_or_else(|| Error::msg("Missing teams array"))?
                .iter()
                .filter_map(|t| t.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>();

            let used_players = request_data["used_players"].as_array()
                .unwrap_or(&vec![])
                .iter()
                .filter_map(|p| p.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>();

            let hint_result = generate_hint(&teams, &used_players)?;
            Ok(Response::from_status(StatusCode::OK)
                .with_content_type(mime::APPLICATION_JSON)
                .with_header("Access-Control-Allow-Origin", "*")
                .with_body(serde_json::to_string(&hint_result).expect("failed to serialize hint result")))
        },
        // Catch all other requests and return a 404.
        _ => Ok(Response::from_status(StatusCode::NOT_FOUND)
            .with_header("Access-Control-Allow-Origin", "*")
            .with_body_text_plain("The page you requested could not be found\n")),
    }
}

// Generate a hint for a player who fits all remaining teams and is not used
fn generate_hint(teams: &[String], used_players: &[String]) -> Result<serde_json::Value, Error> {
    let player_data = get(2)?;
    let mut best_player: Option<serde_json::Value> = None;
    let mut best_count = 0;

    // Find the player who satisfies the most teams and is not used
    if let Some(teams_obj) = player_data["teams"].as_object() {
        // Build a map of team_code -> Vec<PlayerInfo>
        let mut all_players: Vec<serde_json::Value> = Vec::new();
        let mut player_team_map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        for team in teams {
            let team_code = team_code_from_name(team);
            if let Some(code) = team_code {
                if let Some(players_array) = teams_obj.get(code).and_then(|v| v.as_array()) {
                    for p in players_array {
                        let pid = p.get("id").and_then(|id| id.as_str()).unwrap_or("");
                        if !used_players.iter().any(|u| u.eq(pid)) {
                            let entry = player_team_map.entry(pid.to_string()).or_insert_with(Vec::new);
                            if !entry.contains(team) {
                                entry.push(team.clone());
                            }
                            all_players.push(p.clone());
                        }
                    }
                }
            }
        }
        // Find the player who covers the most teams
        let mut checked = std::collections::HashSet::new();
        for p in all_players {
            let pid = p.get("id").and_then(|n| n.as_str()).unwrap_or("").to_string();
            if checked.contains(&pid) { continue; }
            checked.insert(pid.clone());
            let count = player_team_map.get(&pid).map(|v| v.len()).unwrap_or(0);
            if count > best_count {
                best_count = count;
                best_player = Some(p.clone());
            }
        }
    }

    println!("The best player is {:?} who fits {} teams", best_player, best_count);

    let id = best_player.as_ref().and_then(|p| p.get("id")).and_then(|id| id.as_str()).unwrap_or("0");
    let url = format!("https://api-web.nhle.com/v1/player/{}/landing", id);

    println!("Fetching player details from URL: {}", url);
    let body_str =
    Request::get(url)
        .send("nhl-api")?
        .into_body()
        .into_string();
    
    let player_details : serde_json::Value = serde_json::from_str(&body_str)?;

    println!("Details: {:?}", player_details);

    // Generate hints
    let mut hints = Vec::new();
    if let Some(player) = &best_player {
        if best_count < teams.len() {
            hints.push(format!("This player fits {} out of {} teams.", best_count, teams.len()));
        }
        // 1. NHL teams played for
       let teams = get_teams_played_for(id)?;
        if !teams.is_empty() {
            hints.push(format!("Played for NHL teams: {}", teams.join(", ")));
        }

        let seasons = player_details.get("seasonTotals").and_then(|s| s.as_array()).unwrap_or(&vec![]).clone();
        if !seasons.is_empty() {
            for season in seasons.iter().rev() {
                if let Some(league) = season.get("leagueAbbrev").and_then(|l| l.as_str()) {
                    if league != "NHL" {
                        continue;
                    }
                    if let Some(points) = season.get("points").and_then(|p| p.as_i64()) {
                        hints.push(format!("Had {} points in the most recent season.", points));
                        break;
                    } else if let Some(save_pct) = season.get("savePctg").and_then(|p| p.as_number()) {
                        hints.push(format!("Had a save percentage of {:.3} in the most recent season.", save_pct.as_f64().unwrap()));
                        break;
                    }
                }
            }
        }

        player_details.get("birthCountry").and_then(|c| c.as_str()).map(|country| {
            hints.push(format!("Born in {}", country));
        });

        // // 3. Current cap hit
        // if let Some(cap_hit) = player.get("capHit").and_then(|c| c.as_i64()) {
        //     hints.push(format!("Current cap hit: ${}M", cap_hit / 1_000_000));
        // }

        // 4. Amateur team

        // 5. Height/weight
        let height = player_details.get("heightInInches").and_then(|h| h.as_str());
        let weight = player_details.get("weightInPounds").and_then(|w| w.as_i64());
        if let (Some(h), Some(w)) = (height, weight) {
            hints.push(format!("Height/Weight: {} / {} lbs", h, w));
        }

        // 6. Draft position and year
        let draft_details = player_details.get("draftDetails").and_then(|d| d.as_object());

        if let Some(draft_details) = draft_details {
            let (year, round, pick) = (
                draft_details.get("year").and_then(|y| y.as_i64()),
                draft_details.get("round").and_then(|r| r.as_i64()),
                draft_details.get("pickInRound").and_then(|p| p.as_i64())
            );
            if let (Some(y), Some(r), Some(p)) = (year, round, pick) {
                hints.push(format!("Drafted in {}: Round {}, Pick {}", y, r, p));
            }
            if let Some(team) = draft_details.get("teamAbbrev").and_then(|t| t.as_str()) {
                hints.push(format!("Drafted by {}", team));
            }
        }

        // 8. Years active
        let mut first_season = None;
        let mut last_season = None;
        if !seasons.is_empty() {
            for season in &seasons {
                if let Some(league) = season.get("leagueAbbrev").and_then(|l| l.as_str()) {
                    if league != "NHL" {
                        continue;
                    }
                }
                if let Some(year) = season.get("season").and_then(|s| s.as_number()) {
                    let year_str = format!("{}", year.as_u64().unwrap());
                    if first_season.is_none() {
                        first_season = Some(year_str.clone());
                    }
                    last_season = Some(year_str);
                }
            }
            if (first_season.is_some() && last_season.is_some()) {
                hints.push(format!("Played in NHL from {} to {}", first_season.unwrap(), last_season.unwrap()));
            }
        }
        
        // 9. Career points/save percentage
        if let Some(career_totals) = player.get("careerTotals").and_then(|s| s.as_object()) {
            if let Some(regular_season) = career_totals.get("regularSeason").and_then(|r| r.as_object()) {
                if let Some(points) = regular_season.get("points").and_then(|p| p.as_i64()) {
                    hints.push(format!("Career regular season points: {}", points));
                }
                if let Some(save_pct) = regular_season.get("savePctg").and_then(|p| p.as_number()) {
                    hints.push(format!("Career regular season save percentage: {:.3}", save_pct.as_f64().unwrap()));
                }
            }
        }
    }
    
    Ok(serde_json::json!({ "hints": hints }))
}

// Helper: get team code from name
fn team_code_from_name(name: &str) -> Option<&'static str> {
    match name {
        "Anaheim Ducks" => Some("ANA"), "Boston Bruins" => Some("BOS"), "Buffalo Sabres" => Some("BUF"),
        "Calgary Flames" => Some("CGY"), "Carolina Hurricanes" => Some("CAR"), "Chicago Blackhawks" => Some("CHI"),
        "Colorado Avalanche" => Some("COL"), "Columbus Blue Jackets" => Some("CBJ"), "Dallas Stars" => Some("DAL"),
        "Detroit Red Wings" => Some("DET"), "Edmonton Oilers" => Some("EDM"), "Florida Panthers" => Some("FLA"),
        "Los Angeles Kings" => Some("LAK"), "Minnesota Wild" => Some("MIN"), "Montreal Canadiens" => Some("MTL"),
        "Nashville Predators" => Some("NSH"), "New Jersey Devils" => Some("NJD"), "New York Islanders" => Some("NYI"),
        "New York Rangers" => Some("NYR"), "Ottawa Senators" => Some("OTT"), "Philadelphia Flyers" => Some("PHI"),
        "Pittsburgh Penguins" => Some("PIT"), "San Jose Sharks" => Some("SJS"), "Seattle Kraken" => Some("SEA"),
        "St. Louis Blues" => Some("STL"), "Tampa Bay Lightning" => Some("TBL"), "Toronto Maple Leafs" => Some("TOR"),
        "Utah Hockey Club" => Some("UTA"), "Vancouver Canucks" => Some("VAN"), "Vegas Golden Knights" => Some("VGK"),
        "Washington Capitals" => Some("WSH"), "Winnipeg Jets" => Some("WPG"),
        _ => None,
    }
}