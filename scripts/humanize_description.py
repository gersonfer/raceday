import re

# -------------------------------------------------
# Regras declarativas por token
# -------------------------------------------------

TOKEN_RULES = {
    # Eventos
    "gp":        {"label": "GP", "case": "upper"},

    # Categorias / siglas
    "f1":        {"label": "F1", "case": "upper"},
    "gt":        {"label": "GT", "case": "upper"},
    "gt1":       {"label": "GT1", "case": "upper"},
    "gt2":       {"label": "GT2", "case": "upper"},
    "gt3":       {"label": "GT3", "case": "upper"},
    "gt2000":    {"label": "GT2000", "case": "upper"},
    "tc1000":    {"label": "TC1000", "case": "upper"},
    "dtm":       {"label": "DTM", "case": "upper"},
    "dtm-e":     {"label": "DTM-E", "case": "upper", "role": "suffix"},
    "lmh":       {"label": "LMH", "case": "upper"},
    "sp":        {"label": "SP", "case": "upper"},
    "dg":        {"label": "DG", "case": "upper"},
    "brm":       {"label": "BRM", "case": "upper"},

    # Marcas
    "porsche":   {"label": "Porsche", "case": "title"},
    "ferrari":   {"label": "Ferrari", "case": "title"},
    "toyota":    {"label": "Toyota", "case": "title"},
    "nsr":       {"label": "NSR", "case": "upper"},
    "revoslot":  {"label": "Revoslot", "case": "title"},

    # Tipos de evento
    "treino":    {"label": "Treino", "case": "title"},
    "warmup":    {"label": "Warmup", "case": "title"},
    "corrida":   {"label": "Corrida", "case": "title"},
    "campeonato":{"label": "Campeonato", "case": "title"},
    "classico":  {"label": "Clássico", "case": "title"},
    "classicos": {"label": "Clássicos", "case": "title"},
    "hypercar":  {"label": "Hypercar", "case": "title"},

    # Sufixos
    "final":     {"label": "Final", "role": "suffix"},
}

QUALI_TOKENS = {"q1", "q2", "q3"}

# -------------------------------------------------
# Função principal
# -------------------------------------------------

def humanize_description(slug: str) -> str:
    s = slug.lower()
    s = re.sub(r"_+", " ", s).strip()
    tokens = s.split()

    out = []
    suffixes = []
    round_info = None

    i = 0
    while i < len(tokens):
        t = tokens[i]

        # ----------------------------
        # Q1 / Q2 / Q3
        # ----------------------------
        if t in QUALI_TOKENS:
            suffixes.append(t.upper())
            i += 1
            continue

        # ----------------------------
        # Etapa ordinal
        # ----------------------------
        if t.isdigit() and i + 1 < len(tokens) and tokens[i + 1] == "etapa":
            out.append(f"{t}ª Etapa")
            i += 2
            continue

        # ----------------------------
        # Fração (8 8) → (8/8)
        # ----------------------------
        if t.isdigit() and i + 1 < len(tokens) and tokens[i + 1].isdigit():
            round_info = f"{t}/{tokens[i + 1]}"
            i += 2
            continue

        # ----------------------------
        # Token conhecido
        # ----------------------------
        rule = TOKEN_RULES.get(t)
        if rule:
            label = rule["label"]
            if rule.get("role") == "suffix":
                suffixes.append(label)
            else:
                out.append(label)
            i += 1
            continue

        # ----------------------------
        # Palavra genérica
        # ----------------------------
        out.append(t.capitalize())
        i += 1

    # -------------------------------------------------
    # Montagem final
    # -------------------------------------------------

    result = " ".join(out)

    if suffixes:
        result += " – " + " – ".join(suffixes)

    if round_info:
        result += f" ({round_info})"

    return result