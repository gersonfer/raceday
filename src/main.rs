use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fs;
use std::process::{exit, Command};
use tera::{Context, Tera};
use aws_sdk_s3::{Client, primitives::ByteStream};
use std::path::Path;

#[derive(Serialize, Deserialize)]
struct PilotoDisplay {
    nome: String,
    total_laps: i64,
    penalties: i64,
    zona: String,
    gap: String,
    sessions: i64,
    best_time: String,
    average_time: String,
    is_overall_best: bool,
    best_slot_name: String, 
    laps_per_slot: HashMap<String, String>,
    times_per_slot: HashMap<String, String>,
}

// --- INFRAESTRUTURA DE NUVEM (R2) ---

async fn upload_to_r2(file_path: &str, target_key: &str) -> Result<(), Box<dyn Error>> {
    let endpoint = env::var("R2_ENDPOINT").expect("❌ R2_ENDPOINT não definida");
    let bucket = env::var("R2_BUCKET").unwrap_or_else(|_| "raceday-data".to_string());

    let config = aws_config::from_env()
        .endpoint_url(endpoint)
        .region(aws_config::Region::new("auto"))
        .load().await;

    let client = Client::new(&config);
    let body = ByteStream::from_path(Path::new(file_path)).await?;
    
    let content_type = if file_path.ends_with(".html") { "text/html" } else { "application/json" };

    client.put_object()
        .bucket(bucket)
        .key(target_key)
        .body(body)
        .content_type(content_type)
        .send().await?;

    println!("✅ Sincronizado no R2: {}", target_key);
    Ok(())
}

async fn trigger_render_sync() {
    if let Ok(url) = env::var("RENDER_SYNC_URL") {
        let client = reqwest::Client::new();
        // O Render pode demorar para acordar, definimos timeout de 60s
        let _ = client.post(url)
            .timeout(std::time::Duration::from_secs(60))
            .send().await;
        println!("🔔 Notificação de rebuild enviada ao Render.com");
    }
}

// --- LÓGICA DE NEGÓCIO E RELATÓRIO ---

fn gerar_json_grafico(ranking: &Vec<PilotoDisplay>, slots_count: i64) -> String {
    let mut datasets = Vec::new();
    let cores_grafico = vec![
        "#FF6384", "#36A2EB", "#FFCE56", "#4BC0C0", "#9966FF", "#FF9F40", "#8BC34A", "#000000",
        "#E91E63", "#9C27B0", "#00BCD4", "#009688", "#CDDC39", "#FFEB3B", "#795548", "#607D8B"
    ];

    let fenda_nomes_eixo = vec!["Vermelha", "Branca", "Verde", "Laranja", "Azul", "Amarela", "Roxa", "Preta"];

    for (idx, piloto) in ranking.iter().enumerate() {
        let mut data_pontos = Vec::new();
        for s in 1..=slots_count {
            let voltas = piloto.laps_per_slot.get(&s.to_string())
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            data_pontos.push(voltas);
        }

        datasets.push(serde_json::json!({
            "label": piloto.nome,
            "data": data_pontos,
            "borderColor": cores_grafico.get(idx).unwrap_or(&"#CCCCCC"),
            "backgroundColor": cores_grafico.get(idx).unwrap_or(&"#CCCCCC"),
            "fill": false,
            "tension": 0.1
        }));
    }

    let labels_clube: Vec<String> = fenda_nomes_eixo.iter()
        .take(slots_count as usize)
        .map(|s| s.to_string())
        .collect();

    serde_json::json!({
        "labels": labels_clube,
        "datasets": datasets
    }).to_string()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 { eprintln!("❌ Informe o arquivo .INI"); exit(1); }
    let ini_path = &args[1];

    let club = env::var("CLUB").expect("❌ CLUB não definida");
    let track = env::var("TRACK").expect("❌ TRACK não definida");

    // Ajuste o caminho do script conforme sua estrutura
    println!("🚀 [1/5] Iniciando processamento Python (Fidelidade Total)...");
    let output = Command::new("python3")
        .arg("scripts/raceday-prep.py")
        .arg("--input").arg(ini_path)
        .arg("--club").arg(&club)
        .arg("--track").arg(&track)
        .output()?;

    if !output.status.success() {
        eprintln!("⚠️ Erro no preparador: {}", String::from_utf8_lossy(&output.stderr));
        exit(1);
    }

    let data: Value = serde_json::from_slice(&output.stdout)?;
    let ts = data["event"]["timestamp"].as_str().unwrap_or("000");

    // --- PROCESSAMENTO DO RANKING ---
    let mut ranking: Vec<PilotoDisplay> = Vec::new();
    let mut best_lap_overall = 999.999;
    let fenda_nomes = vec!["", "Vermelha", "Branca", "Verde", "Laranja", "Azul", "Amarela", "Roxa", "Preta"];

    if let Some(pilots_map) = data["pilots"].as_object() {
        for (id, p_info) in pilots_map {
            let mut laps_map = HashMap::new();
            let mut times_map = HashMap::new();
            let mut total_voltas = 0;
            let mut melhor_tempo_piloto = 999.999;
            let mut best_slot_idx = 1;
            let mut sessions_count = 0;

            if let Some(races) = data["races"].as_array() {
                for race in races {
                    if let Some(sessions) = race["sessions"].as_array() {
                        for session in sessions {
                            if let Some(slots) = session["slots"].as_object() {
                                for (slot_idx, s_data) in slots {
                                    if s_data["p_id"].to_string() == format!("\"{}\"", id) || s_data["p_id"].to_string() == *id {
                                        let l = s_data["laps"].as_i64().unwrap_or(0);
                                        let t = s_data["best"].as_f64().unwrap_or(0.0);
                                        if l > 0 { sessions_count += 1; }
                                        total_voltas += l;
                                        laps_map.insert(slot_idx.clone(), l.to_string());
                                        times_map.insert(slot_idx.clone(), if t > 0.0 { format!("{:.3}", t) } else { "---".into() });
                                        if t > 0.0 && t < melhor_tempo_piloto {
                                            melhor_tempo_piloto = t;
                                            best_slot_idx = slot_idx.parse().unwrap_or(1);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if melhor_tempo_piloto < best_lap_overall && melhor_tempo_piloto > 0.0 { best_lap_overall = melhor_tempo_piloto; }
            let display_best = if melhor_tempo_piloto >= 900.0 { "0.000".into() } else { format!("{:.3}", melhor_tempo_piloto) };
            
            // Aqui corrigimos para usar o total_laps OFICIAL do ranking se disponível
            let mut final_laps = total_voltas;
            let mut final_gap = "0".to_string();
            // let mut final_zona = "000".to_string();

            if let Some(off_rank) = data["official_ranking"].as_array() {
                if let Some(p_off) = off_rank.iter().find(|x| x["p_id"].as_str().unwrap_or("") == id) {
                    final_laps = p_off["laps"].as_i64().unwrap_or(total_voltas);
                    final_gap = p_off["gap"].as_str().unwrap_or("0").to_string();
                }
            }

            let media = if sessions_count > 0 { final_laps as f64 / sessions_count as f64 } else { 0.0 };

            ranking.push(PilotoDisplay {
                nome: p_info["name"].as_str().unwrap_or("---").to_string(),
                total_laps: final_laps,
                penalties: data["raw_results"]["penaltys"][id].as_i64().unwrap_or(0),
                zona: data["raw_results"]["zones"][id].as_str().unwrap_or("000").to_string(),
                gap: final_gap,
                sessions: sessions_count,
                best_time: display_best,
                average_time: format!("{:.1}", media).replace(".", ","),
                is_overall_best: false,
                best_slot_name: fenda_nomes.get(best_slot_idx as usize).unwrap_or(&"---").to_string(),
                laps_per_slot: laps_map,
                times_per_slot: times_map,
            });
        }
    }

    ranking.sort_by(|a, b| b.total_laps.cmp(&a.total_laps));
    let best_lap_str = format!("{:.3}", best_lap_overall);
    for p in &mut ranking { if p.best_time == best_lap_str && best_lap_overall < 900.0 { p.is_overall_best = true; } }

    // --- CÁLCULO MELHORES TEMPOS POR SLOT ---
    let mut best_times_per_slot: HashMap<String, String> = HashMap::new();
    for p in &ranking {
        for (slot, time_str) in &p.times_per_slot {
            if let Ok(t) = time_str.parse::<f64>() {
                let current_best_str = best_times_per_slot.get(slot).cloned().unwrap_or("999.999".to_string());
                let current_best = current_best_str.parse::<f64>().unwrap_or(999.999);
                if t < current_best && t > 0.0 {
                    best_times_per_slot.insert(slot.clone(), format!("{:.3}", t));
                }
            }
        }
    }

    // --- TERA TEMPLATES ---
    println!("📄 [2/5] Gerando relatório HTML...");
    let mut tera = Tera::default();
    tera.add_template_file("templates/report.html", Some("report"))?;
    let mut context = Context::new();

    let mut insights = Vec::new();
    if let Some(top) = ranking.iter().find(|p| p.is_overall_best) {
        insights.push(format!("🔥 Melhor Volta Absoluta: Performance de elite por {} com o tempo de {}s na fenda {}.", top.nome, top.best_time, top.best_slot_name));
    }
    if ranking.len() > 0 {
        insights.push(format!("🏆 Domínio técnico: O vencedor {} demonstrou consistência extrema, completando {} voltas.", ranking[0].nome, ranking[0].total_laps));
    }

    context.insert("insights", &insights);
    context.insert("best_times_per_slot", &best_times_per_slot);
    context.insert("overall_best_time_formatted", &best_lap_str);
    context.insert("club", &club); 
    context.insert("track", &track);
    context.insert("event", &data["event"]); 
    context.insert("metadata", &data["metadata"]);
    context.insert("ranking_display", &ranking); 
    context.insert("dados_grafico", &gerar_json_grafico(&ranking, data["metadata"]["slots"].as_i64().unwrap_or(6)));

    let html_output = tera.render("report", &context)?;
    
    // --- SALVAMENTO E UPLOAD ---
    // Criamos identificadores limpos para os nomes dos arquivos
    let club_slug = club.to_lowercase().replace(" ", "_");
    let track_slug = track.to_lowercase().replace(" ", "_");
    let race_slug = data["event"]["slug"].as_str().unwrap_or("race");

    // O JSON agora é ÚNICO por clube e pista: races/clube_pista_timestamp.json
    let r2_key_json = format!("races/{}_{}_{}.json", club_slug, track_slug, ts);
    
    // O HTML segue o padrão: reports/clube_pista_corrida_timestamp.html
    let r2_key_html = format!("reports/{}_{}_{}_{}.html", club_slug, track_slug, race_slug, ts);
    
    fs::create_dir_all("temp_out")?;
    let local_json_path = format!("temp_out/last_upload.json");
    let local_html_path = format!("temp_out/last_upload.html");
    
    fs::write(&local_json_path, serde_json::to_string_pretty(&data)?)?;
    fs::write(&local_html_path, &html_output)?;

    println!("☁️ [3/5] Enviando JSON para o R2: {}", r2_key_json);
    upload_to_r2(&local_json_path, &r2_key_json).await?;

    println!("☁️ [4/5] Enviando HTML para o R2: {}", r2_key_html);
    upload_to_r2(&local_html_path, &r2_key_html).await?;

    println!("🔔 [5/5] Sincronizando com Render.com...");
    trigger_render_sync().await;

    println!("\n✨ Processo concluído com sucesso!");
    Ok(())
}