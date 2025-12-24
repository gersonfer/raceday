#!/usr/bin/env python3
# -*- coding: utf-8 -*-

import argparse
import configparser
import json
import os
import re
import sys
import io
import unicodedata
from datetime import datetime, timezone

# 1. FORÇA O TERMINAL A ACEITAR UTF-8
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')
sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding='utf-8')

def slugify(text: str) -> str:
    """
    Remove acentuação corretamente:
    'CONFRATERNIZAÇÃO' -> 'confraternizacao'
    """
    if not text:
        return "corrida"

    # Normalização compatível (mais segura que NFD)
    text = unicodedata.normalize("NFKD", text)

    # Remove qualquer acento de forma definitiva
    text = text.encode("ascii", "ignore").decode("ascii")

    # Limpeza final
    text = re.sub(r"[^\w\s-]", "", text)
    text = text.strip().replace(" ", "_").lower()

    return text

def parse_int(value: str) -> int:
    try: return int(value)
    except: return 0

def centiseconds_to_seconds(raw: int) -> float:
    return raw / 10000.0 if raw > 0 else 0.0

def read_ini_with_fallback(path: str) -> configparser.ConfigParser:
    config = configparser.ConfigParser(strict=False, interpolation=None)

    try:
        with open(path, "rb") as f:
            raw_data = f.read()

        # 1️⃣ tenta UTF-8 primeiro (correto em 2025)
        try:
            text = raw_data.decode("utf-8")
        except UnicodeDecodeError:
            # 2️⃣ fallback real para arquivos Windows antigos
            text = raw_data.decode("cp1252")

        config.read_string(text)
        return config

    except Exception as e:
        print(f"ERRO CRÍTICO: Falha ao ler arquivo: {e}", file=sys.stderr)
        sys.exit(2)

def main() -> None:
    parser = argparse.ArgumentParser(description="SlotChrono INI to Fidelidade-Total JSON")
    parser.add_argument("--input", required=True)
    parser.add_argument("--track", required=True)
    parser.add_argument("--club", required=True)
    parser.add_argument("--output", default="stdout")
    args = parser.parse_args()

    config = read_ini_with_fallback(args.input)

    # --- 1. VALIDAÇÃO DE INTEGRIDADE ---
    secoes_obrigatorias = ["config", "pilots", "races", "gp_result_pilots", "gp_result_laps"]
    faltando = [s for s in secoes_obrigatorias if s not in config]
    
    if faltando or "race_1_1" not in config:
        print(f"ERRO DE PADRÃO: Arquivo incompleto. Faltam: {', '.join(faltando)}", file=sys.stderr)
        sys.exit(3)

    # --- 2. METADADOS E SLUGS ---
    # Pegamos o nome original com acentos
    raw_name = config.get("config", "name", fallback="Corrida").strip('"')
    display_title = raw_name
    # Geramos o slug limpo (sem acentos) para o nome do arquivo/URL
    slug = slugify(display_title)

    filename_input = os.path.basename(args.input)
    match_ts = re.search(r'(\d{14})', filename_input)
    ini_timestamp = match_ts.group(1) if match_ts else datetime.now().strftime("%Y%m%d%H%M%S")
    
    # Detecção dinâmica de slots
    max_slot = 0
    slot_pattern = re.compile(r"slot_(\d+)_")
    for section in config.sections():
        for key in config[section].keys():
            m = slot_pattern.match(key)
            if m: max_slot = max(max_slot, int(m.group(1)))

    # --- 3. ESTRUTURA DO JSON ---
    result = {
        "org_car_version": "1.1",
        "club": args.club.upper(),
        "track": args.track.upper(),
        "event": {
            "title": display_title,
            "slug": slug,
            "date_ini": config.get("config", "date", fallback=""),
            "timestamp": ini_timestamp
        },
        "metadata": {
            "slots": max_slot,
            "generated_at": datetime.now(timezone.utc).isoformat(),
        },
        "official_ranking": [], 
        "pilots": {k: {"name": v.strip('"')} for k, v in config["pilots"].items()},
        "races": [],
        "raw_results": {
            "laps": dict(config["gp_result_laps"]),
            "best_times": dict(config["gp_result_best_times"]),
            "gaps": dict(config["gp_result_gap"]) if "gp_result_gap" in config else {},
            "zones": dict(config["gp_result_zone"]) if "gp_result_zone" in config else {},
            "penaltys": dict(config["gp_result_penaltys"]) if "gp_result_penaltys" in config else {}
        }
    }

    # --- 4. CONSTRUÇÃO DO RANKING OFICIAL ---
    for p_id, p_name in config["gp_result_pilots"].items():
        best_raw = parse_int(config.get("gp_result_best_times", p_id, fallback="0"))
        result["official_ranking"].append({
            "p_id": p_id,
            "name": p_name.strip('"'),
            "laps": parse_int(config.get("gp_result_laps", p_id, fallback="0")),
            "gap": config.get("gp_result_gap", p_id, fallback="0"),
            "best_lap": centiseconds_to_seconds(best_raw)
        })

    # --- 5. SESSÕES DE BATERIA ---
    race_section_re = re.compile(r"race_(\d+)_(\d+)")
    race_sessions = {}
    for section in config.sections():
        m = race_section_re.fullmatch(section)
        if m:
            r_id, s_id = int(m.group(1)), int(m.group(2))
            race_sessions.setdefault(r_id, []).append(s_id)

    for r_id in sorted(race_sessions.keys()):
        race_obj = {"race_id": r_id, "name": f"Bateria {r_id}", "sessions": []}
        for s_id in sorted(race_sessions[r_id]):
            sec_name = f"race_{r_id}_{s_id}"
            sec = config[sec_name]
            session_obj = {"session": s_id, "slots": {}}
            for slot in range(1, max_slot + 1):
                prefix = f"slot_{slot}_"
                pname = sec.get(prefix + "pilot_name", "").strip()
                if pname:
                    p_id = sec.get(prefix + "pilot_number", "0")
                    best_raw = parse_int(sec.get(prefix + "best", "0"))
                    session_obj["slots"][str(slot)] = {
                        "p_id": p_id,
                        "name": pname,
                        "laps": parse_int(sec.get(prefix + "laps", "0")),
                        "best": centiseconds_to_seconds(best_raw)
                    }
            race_obj["sessions"].append(session_obj)
        result["races"].append(race_obj)

    # --- SAÍDA ---
    json_output = json.dumps(result, ensure_ascii=False, indent=2)
    if args.output == "stdout":
        print(json_output)
    else:
        with open(args.output, "w", encoding="utf-8") as f:
            f.write(json_output)

if __name__ == "__main__":
    main()