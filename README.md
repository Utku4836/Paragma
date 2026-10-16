# Paragma

Ajanlar için yerel hafıza CLI'ı. Tek binary `pg`, Markdown dosyaları tek gerçek kaynak, SQLite FTS5 yalnızca indeks. Ajan hafızayı her mesajda taşımaz; gerektiğinde bölüm bölüm çeker.

Tasarım: [docs/TASARIM.md](docs/TASARIM.md)

## Durum

- **Faz 1 (okuma) tamam:** `init`, `reindex`, artımlı senkron, adres çözücü, `search`, `outline`, `read`, `peek`, `links`, `backlinks`, `find`, `status`.
- **Faz 2 (yazma ve görev) tamam:**
  - yazma: `new`, `append`, `write`, `insert`, `edit`, `section add`, `link`, `mv`, `archive`, `rm`
  - `task`: `add`, `list`, `next`, `done`, …
  - `session`: `start`, `plan`, `log`, `checkpoint`, `resume`, `fork`, `close`, …
  - `project`, `branch`
  - işaretleme: `mark`, `highlight`, `callout`, `unmark`, `marks`
- **Karşılaştırma testi:** Paragma'lı, düz dosyalı ve hafızasız koşullar için [bench/](bench/README.md).

Uzun iş akışı:

```sh
pg session resume                         # yeni konuşmada: hedef, ilerleme, açık task'lar, devir notu
pg session start "Ödeme akışı" --goal "Stripe ile kart ödemesi"
pg session plan "Webhook" "İmza doğrulama" "Testler"     # her argüman bir task, ID'leri döner
pg task done t3f2                         # sıradaki task'ı da söyler
pg session log "karar: indirim vergiden önce uygulanır"
pg session checkpoint "Webhook bitti; sıradaki: testler"
```

## Kurulum

```sh
go build -o bin/pg ./cmd/pg      # Windows: bin/pg.exe
```

## Hızlı bakış

```sh
pg init                                   # .paragma/ oluşturur
pg search "database locked"               # bölüm düzeyinde arama: adres + snippet + token
pg outline architecture                   # başlık iskeleti, her başlığın token maliyeti
pg read architecture#Middleware           # yalnızca o başlık
pg read auth-refactor^t002                # tek blok / task
pg read stack:10-24                       # satır aralığı
pg backlinks jwt-exp-timezone             # gelen bağlantılar
pg find --type error --since 30d          # metadata filtresi
```

Adresler: `not`, `not#Başlık`, `not#Üst/Alt`, `not^blok`, `^blok`, `not:10-24`, `@ULID`, `~not` (global vault).

Çıkış kodları: 0 başarılı, 1 sonuç yok, 2 kullanım hatası, 3 adres bulunamadı, 5 vault yok, 7 iç hata.

## Geliştirme

```sh
go test ./...
go run ./tools/gen -n 500 -out /tmp/bench    # performans için sentetik vault
PARAGMA_TRACE=1 pg search x                  # aşama süreleri (stderr)
```
