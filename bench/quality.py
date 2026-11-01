"""Search quality of `pg find` (Paragma 2) against `pg context` (the Go
version), on the same memory (bench/memory-big) and the same fact checks
as bench/quick.py.

Question sets:
  topic       the topics agents ask about (quick.py's questions)
  paraphrase  the same needs in other words
  turkish     the same needs in Turkish (also spanish, german, french, portuguese)
  absent      things memory does not hold: the right answer is nothing
  held-*      a held-out set no tuning looks at: a change must not make it worse

    python bench/quality.py [--budget N] [--go]   (--go also runs the Go version)
"""
import argparse
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
EXE = ".exe" if os.name == "nt" else ""
PG2 = os.path.join(ROOT, "target", "release", "pg" + EXE)
PG_GO = os.path.join(ROOT, "bin", "pg" + EXE)
sys.path.insert(0, HERE)

FACTS = {
    # the held-out set: facts no tuning looked at
    "wrap": [r"(?i)fmt\.Errorf|operation name"],
    "logreturn": [r"(?i)do not log and return|caller logs"],
    "ids": [r"ULID"],
    "secrets": [r"(?i)environment only|\.env"],
    "logs": [r"(?i)JSON lines", r"(?i)request bodies|PII"],
    "flags": [r"flags\.yaml"],
    "releases": [r"vMAJOR"],
    "durations": [r"(?i)duration strings|\"30s\""],
    "packages": [r"(?i)singular"],
    "init": [r"init\(\)"],
    "golden": [r"testdata", r"(?i)line endings"],
    "database": [r"(?i)postgres"],
    "sentinel": [r"ErrXxx"],
    "ci": [r"golangci-lint|go vet"],
    "refund": [r"30 days", r"ErrRefundWindow", r"audit\.Record", r"ErrInvalidAmount|exceed the paid amount"],
    "checksum": [r"(?i)line endings|\\r\\n", r"gitattributes", r"(?i)do not change ExpectedChecksum"],
    "locked": [r"WAL", r"busy_timeout"],
    "tests": [r"(?i)table tests", r"t\.Cleanup", r"in-package fakes", r"(?i)regression test"],
    "vat": [r"(?i)half up", r"5000\)/10000"],
    "money": [r"int64", r"float64"],
    "discount": [r"(?i)server side"],
    "provider": [r"Iyzico", r"(?i)fake"],
    "time": [r"Europe/Istanbul", r"clock\.Now"],
    "migrations": [r"(?i)forward only", r"(?i)duplicate migration"],
}

# (set, facts, query, Go command for the topic set)
QUESTIONS = [
    ("topic", "refund", "refund", ["context", "refund"]),
    ("topic", "checksum", "catalog_test.go:8: catalog checksum mismatch: got 8c77cb095cbd want 4ff52f25fc9d",
     ["error", "find", "catalog_test.go:8: catalog checksum mismatch: got 8c77cb095cbd want 4ff52f25fc9d"]),
    ("topic", "locked", "database is locked (5)", ["error", "find", "database is locked (5)"]),
    ("topic", "tests", "writing tests", ["context", "writing tests"]),
    ("topic", "vat", "vat rounding", ["context", "vat rounding"]),
    ("topic", "money", "money amounts", ["context", "money amounts"]),
    ("topic", "discount", "discount codes", ["context", "discount codes"]),
    ("topic", "provider", "payment provider", ["context", "payment provider"]),
    ("topic", "time", "time zone utc", ["context", "time zone utc"]),
    ("topic", "migrations", "migrations", ["context", "migrations"]),
    ("paraphrase", "refund", "how long after a purchase can a customer get their money back", None),
    ("paraphrase", "checksum", "the product list test fails on windows with a hash mismatch", None),
    ("paraphrase", "locked", "sqlite fails when two tests write at the same time", None),
    ("paraphrase", "tests", "what conventions should a new unit test follow", None),
    ("paraphrase", "vat", "how do we round tax on prices", None),
    ("paraphrase", "money", "which type should hold a price in code", None),
    ("paraphrase", "discount", "can the browser compute the final price of a coupon", None),
    ("paraphrase", "provider", "which company processes card charges and how is it mocked", None),
    ("paraphrase", "time", "how should dates and clocks be handled", None),
    ("paraphrase", "migrations", "how do I change the database schema", None),
    ("turkish", "refund", "para iadesi ödemeden sonra kaç gün içinde yapılabilir", None),
    ("turkish", "checksum", "windowsta katalog testi checksum hatası veriyor", None),
    ("turkish", "locked", "veritabanı kilitli hatası paralel testlerde", None),
    ("turkish", "tests", "test yazarken hangi kurallara uyulmalı", None),
    ("turkish", "vat", "KDV nasıl yuvarlanır", None),
    ("turkish", "money", "para tutarları kodda hangi tiple tutulur", None),
    ("turkish", "discount", "indirim kodları nerede doğrulanır", None),
    ("turkish", "provider", "ödeme sağlayıcısı hangisi ve testlerde nasıl taklit edilir", None),
    ("turkish", "time", "saat dilimi ve zaman kuralları", None),
    ("turkish", "migrations", "veritabanı şema değişikliği kuralları", None),
    ("spanish", "refund", "cuántos días después del pago puede un cliente pedir un reembolso", None),
    ("spanish", "checksum", "la prueba del catálogo falla en Windows con un checksum distinto", None),
    ("spanish", "locked", "la base de datos está bloqueada cuando las pruebas corren en paralelo", None),
    ("spanish", "tests", "qué reglas siguen las pruebas unitarias en este proyecto", None),
    ("spanish", "vat", "cómo se redondea el IVA", None),
    ("spanish", "money", "qué tipo se usa para los importes de dinero en el código", None),
    ("spanish", "discount", "dónde se validan los códigos de descuento", None),
    ("spanish", "provider", "qué proveedor de pagos usamos y cómo se simula en las pruebas", None),
    ("spanish", "time", "cómo se manejan las zonas horarias y la hora actual", None),
    ("spanish", "migrations", "reglas para cambiar el esquema de la base de datos", None),
    ("german", "refund", "wie viele Tage nach der Zahlung ist eine Rückerstattung erlaubt", None),
    ("german", "checksum", "der Katalogtest schlägt unter Windows mit falscher Prüfsumme fehl", None),
    ("german", "locked", "Datenbank ist gesperrt wenn Tests parallel laufen", None),
    ("german", "tests", "welche Regeln gelten für Unit-Tests in diesem Projekt", None),
    ("german", "vat", "wie wird die Mehrwertsteuer gerundet", None),
    ("german", "money", "welcher Typ wird für Geldbeträge im Code verwendet", None),
    ("german", "discount", "wo werden Rabattcodes geprüft", None),
    ("german", "provider", "welcher Zahlungsanbieter wird genutzt und wie wird er in Tests ersetzt", None),
    ("german", "time", "wie werden Zeitzonen und die aktuelle Uhrzeit behandelt", None),
    ("german", "migrations", "Regeln für Änderungen am Datenbankschema", None),
    ("french", "refund", "combien de jours après le paiement un client peut-il être remboursé", None),
    ("french", "checksum", "le test du catalogue échoue sous Windows avec une somme de contrôle différente", None),
    ("french", "locked", "la base de données est verrouillée quand les tests tournent en parallèle", None),
    ("french", "tests", "quelles règles suivent les tests unitaires de ce projet", None),
    ("french", "vat", "comment la TVA est-elle arrondie", None),
    ("french", "money", "quel type sert pour les montants d'argent dans le code", None),
    ("french", "discount", "où les codes de réduction sont-ils validés", None),
    ("french", "provider", "quel prestataire de paiement utilisons-nous et comment est-il simulé dans les tests", None),
    ("french", "time", "comment gérer les fuseaux horaires et l'heure actuelle", None),
    ("french", "migrations", "règles pour modifier le schéma de la base de données", None),
    ("portuguese", "refund", "quantos dias depois do pagamento um cliente pode pedir reembolso", None),
    ("portuguese", "checksum", "o teste do catálogo falha no Windows com checksum diferente", None),
    ("portuguese", "locked", "o banco de dados fica bloqueado quando os testes rodam em paralelo", None),
    ("portuguese", "tests", "quais regras os testes unitários deste projeto seguem", None),
    ("portuguese", "vat", "como o imposto sobre valor agregado é arredondado", None),
    ("portuguese", "money", "que tipo é usado para valores em dinheiro no código", None),
    ("portuguese", "discount", "onde os códigos de desconto são validados", None),
    ("portuguese", "provider", "qual provedor de pagamento usamos e como ele é simulado nos testes", None),
    ("portuguese", "time", "como lidar com fusos horários e a hora atual", None),
    ("portuguese", "migrations", "regras para alterar o esquema do banco de dados", None),
    ("held-en", "wrap", "how should errors be wrapped", None),
    ("held-en", "logreturn", "should I log an error before returning it", None),
    ("held-en", "ids", "what format do public IDs use in the API", None),
    ("held-en", "secrets", "where do secrets come from", None),
    ("held-en", "logs", "how are log lines formatted", None),
    ("held-en", "flags", "where are feature flags configured", None),
    ("held-en", "releases", "how are releases tagged", None),
    ("held-en", "durations", "how to write durations in the config", None),
    ("held-en", "packages", "naming rules for Go packages", None),
    ("held-en", "init", "can I use init functions", None),
    ("held-en", "golden", "where do golden files live", None),
    ("held-en", "database", "which database does the API service use", None),
    ("held-en", "sentinel", "how are sentinel errors named", None),
    ("held-en", "ci", "what runs in CI", None),
    ("held-other", "wrap", "hatalar nasıl sarmalanmalı", None),
    ("held-other", "secrets", "gizli anahtarlar nereden okunur", None),
    ("held-other", "packages", "cómo se nombran los paquetes", None),
    ("held-other", "golden", "dónde viven los archivos golden", None),
    ("held-other", "logs", "wie werden Logzeilen formatiert", None),
    ("held-other", "releases", "comment les versions sont-elles étiquetées", None),
    # written after the glossary (src/search/glossary.txt): kept apart in the report
    ("held-late", "ids", "API'de kimlikler hangi biçimde", None),
    ("held-late", "flags", "dónde se configuran las banderas de funciones", None),
    ("held-late", "durations", "wie schreibt man Zeitdauern in der Konfiguration", None),
    ("held-late", "init", "peut-on utiliser des fonctions init", None),
    ("held-late", "ci", "o que roda na integração contínua", None),
    ("held-late", "database", "quale database usa il servizio API", None),
    ("held-late", "sentinel", "sentinel hataları nasıl adlandırılır", None),
    ("held-late", "logreturn", "debo registrar un error antes de devolverlo", None),
    ("held-late", "secrets", "woher kommen die Geheimnisse", None),
    ("held-late", "golden", "où vivent les fichiers golden", None),
    ("held-absent", None, "best pizza dough hydration", None),
    ("held-absent", None, "react native navigation stack", None),
    ("held-absent", None, "docker compose volume mounts", None),
    ("held-absent", None, "ışık hızı saniyede kaç kilometre", None),
    ("absent", None, "how do I bake sourdough bread", None),
    ("absent", None, "kubernetes horizontal pod autoscaler settings", None),
    ("absent", None, "ekmek hamuru nasıl mayalanır", None),
    ("absent", None, "unity shader graph render pipeline", None),
    ("absent", None, "zzkq vrwx", None),
]


LANG_SETS = ("topic", "paraphrase", "turkish", "spanish", "german", "french", "portuguese")
HELD_SETS = ("held-en", "held-other", "held-late")


def tokens(s):
    return (len(s) * 2 + 6) // 7


def run(args, env):
    t = time.perf_counter()
    p = subprocess.run(args, capture_output=True, text=True, encoding="utf-8", errors="replace", env=env)
    return p.stdout, (time.perf_counter() - t) * 1000, p.returncode


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--budget", type=int, default=1000)
    ap.add_argument("--go", action="store_true", help="also run the Go version (bin/pg)")
    ap.add_argument("--show", help="print the output of one set, e.g. --show turkish")
    a = ap.parse_args()

    import to_v2
    home2 = tempfile.mkdtemp(prefix="pgq2-")
    to_v2.convert(os.path.join(HERE, "memory-big"), home2, "shopkit")
    env2 = dict(os.environ, PARAGMA_HOME=home2, PARAGMA_PROJECT="shopkit")
    run([PG2, "doctor"], env2)  # first index
    # every question is its own conversation: pg find shortens what a
    # conversation was already shown
    ask = lambda q: run([PG2, "find", q, "--budget", str(a.budget)], dict(env2, PARAGMA_SESSION=q))
    systems = [("pg 2", lambda q, g: ask(q))]

    if a.go:
        import agent_eval as ae
        home_go = tempfile.mkdtemp(prefix="pgqgo-")
        ae.to_space(ae.MEMORIES["big"], home_go)
        env_go = dict(os.environ, PARAGMA_HOME=home_go)
        env_go.pop("PARAGMA_PROJECT", None)
        run([PG_GO, "prime"], env_go)
        systems.insert(0, ("go pg", lambda q, g: run([PG_GO] + (g or ["context", q]) + ["--again"], env_go)))

    rows = []
    for name, call in systems:
        for st, key, q, g in QUESTIONS:
            out, ms, code = call(q, g)
            if a.show == st and name == "pg 2":
                print(f"--- {q}\n{out}")
            hit = [bool(re.search(f, out)) for f in FACTS[key]] if key else []
            rows.append((name, st, key or q, sum(hit), len(hit), tokens(out) if code == 0 else 0, ms, code))
    shutil.rmtree(home2, ignore_errors=True)

    print(f"\nbudget {a.budget} tokens for pg 2\n")
    print("| system | set | facts found | full answers | tokens (mean) | ms (median) |")
    print("|---|---|---|---|---|---|")
    for name, _ in systems:
        for st in LANG_SETS:
            r = [x for x in rows if x[0] == name and x[1] == st]
            print(f"| {name} | {st} | {sum(x[3] for x in r)}/{sum(x[4] for x in r)} | {sum(1 for x in r if x[3] == x[4])}/{len(r)} | "
                  f"{statistics.mean(x[5] for x in r):,.0f} | {statistics.median(x[6] for x in r):,.0f} |")
        r = [x for x in rows if x[0] == name and x[1] == "absent"]
        quiet = sum(1 for x in r if x[7] == 1)
        print(f"| {name} | absent | nothing found in {quiet}/{len(r)} | | {statistics.mean(x[5] for x in r):,.0f} | |")
        r = [x for x in rows if x[0] == name and x[1] in LANG_SETS]
        print(f"| {name} | **all** | **{sum(x[3] for x in r)}/{sum(x[4] for x in r)}** | {sum(1 for x in r if x[3] == x[4])}/{len(r)} | "
              f"{statistics.mean(x[5] for x in r):,.0f} | |")
        for st in HELD_SETS:
            r = [x for x in rows if x[0] == name and x[1] == st]
            print(f"| {name} | {st} | {sum(x[3] for x in r)}/{sum(x[4] for x in r)} | {sum(1 for x in r if x[3] == x[4])}/{len(r)} | "
                  f"{statistics.mean(x[5] for x in r):,.0f} | {statistics.median(x[6] for x in r):,.0f} |")
        r = [x for x in rows if x[0] == name and x[1] == "held-absent"]
        quiet = sum(1 for x in r if x[7] == 1)
        print(f"| {name} | held-absent | nothing found in {quiet}/{len(r)} | | {statistics.mean(x[5] for x in r):,.0f} | |")
    print("\nmissed:")
    for x in rows:
        if "absent" not in x[1] and x[3] < x[4]:
            print(f"  {x[0]:6} {x[1]:10} {x[2]:10} {x[3]}/{x[4]}")


if __name__ == "__main__":
    main()
