# Paragma

Ajanlar için yerel hafıza CLI'ı. Tek binary `pg`, Markdown dosyaları tek gerçek kaynak, SQLite FTS5 yalnızca indeks. Ajan hafızayı her mesajda taşımaz; gerektiğinde bölüm bölüm çeker. Hafıza `~/.paragma/Spaces/` altında durur: her space (Obsidian'daki vault gibi) kendi proje klasörlerini, aramasını ve grafını taşır.

Tasarım: [docs/TASARIM.md](docs/TASARIM.md)

## Durum

- **Faz 1 (okuma) tamam:** `init`, `reindex`, artımlı senkron, adres çözücü, `search`, `outline`, `read`, `peek`, `links`, `backlinks`, `find`, `status`.
- **Faz 2 (yazma ve görev) tamam:**
  - yazma: `new`, `append`, `write`, `insert`, `edit`, `section add`, `link`, `mv`, `archive`, `rm`
  - `task`: `add`, `list`, `next`, `done`, …
  - `session`: `start`, `plan`, `log`, `checkpoint`, `resume`, `fork`, `close`, … (Faz 6'da proje komutları oldu)
  - `project`, `branch` (Faz 6'da dal katmanı kaldırıldı)
  - işaretleme: `mark`, `highlight`, `callout`, `unmark`, `marks`
- **Faz 3 (episodik + ajan) tamam, MVP:**
  - hata hafızası: `error record`, `find`, `seen`, `reopen`
  - `pg run -- <komut>`: başarısız olursa bilinen çözümü ekler
  - `decision record`, `journal`, `rule add`, `rule list`, `promote`, `supersede`, `verify`, `stale`
  - `prime`, `index`, `skill`, `install claude|agents-md|gemini`, isteğe bağlı hook'lar
  - `grep`, `recent`, `doctor`
- **Faz 4 (ajan için graf) tamam:**
  - `related <dosya>`: o dosyanın kuralları, kararları, geçmiş hataları
  - `context <konu>`: tek çağrılık bağlam paketi
  - `neighbors` / `graph --text`, `orphans`, `broken`
  - proje hafızası git'e girmez (`init` `.gitignore`'a ekler) ve boş başlar
  - global hafıza (Faz 6'da kaldırıldı: space içindeki tüm projeler aranıyor)
- **Faz 5 tamam (modelsiz):** `keywords`, sonuç yokken arama ipucu, `misses`, `dupes`, otomatik terfi, `gc`
- **Karşılaştırma testi:** [bench/RESULTS.md](bench/RESULTS.md). Hafıza doğruluğu belirliyor: hafızasız ajan iki görevi yanlış yaptı, düz notlarla ve Paragma ile hepsi doğru. Düz notlara göre Paragma tekrar eden hatada belirgin şekilde verimli, diğer görevlerde fark küçük ya da yok. Düzenek: [bench/](bench/README.md)
- **Faz 6 (space yapısı ve verimlilik):**
  - hafıza `~/.paragma/Spaces/<space>/<proje>/` altında; repo içinde `.paragma/` yok, global hafıza ve dal katmanı kaldırıldı
  - `start`, `resume`, `plan`, `done`, `log`, `decide`, `checkpoint`; `project list|status|close`; `space list|new|use`
  - `context`: notları bir arada ve küçükleri tam verir, proje kurallarını ekler, sonda kapsam satırı basar

Bir ajana öğretmek için:

```sh
pg install agents-md          # AGENTS.md'ye "Memory (Paragma)" bölümü (Antigravity, Codex, pi …)
pg install claude --hooks     # Claude Code: skill + oturum başı prime + sıkıştırma öncesi checkpoint
```

Proje akışı:

```sh
pg prime                                  # sohbet başı: hafızada ne var, hangi projeler
pg start payments --goal "Stripe ile kart ödemesi" --step "Webhook" --step "Testler" --decision "İndirim KDV'den önce"
pg done t3f2                              # görevi bitir, sıradakini tam olarak verir
pg log "webhook ham gövdeyi istiyor"      # bulgu
pg decide "Yeniden deneme 5 kez"          # kullanıcının kararı
pg new note "Webhook bulguları" --dir webhook   # proje klasörünü ajan düzenler
pg resume payments                        # yeni sohbette ya da projeler arası geçişte
```

Hata hafızası:

```sh
pg run -- go test ./...       # başarısızsa bilinen çözüm; geçerse değişen dosyaların kuralları
pg error record --from-last --cause "CRLF checkout" --fix "Load içinde satır sonlarını normalize et"
pg error find "database is locked (5)"
```

## Kurulum

```sh
go build -o bin/pg ./cmd/pg      # Windows: bin/pg.exe
```

## Hızlı bakış

```sh
pg context refund                         # bir konuda hafızadaki her şey, tek çağrıda
pg search "database locked"               # bölüm düzeyinde arama: adres + snippet + token
pg outline                                # space haritası: projeler ve notlar
pg read shopkit/architecture#Middleware   # yalnızca o başlık
pg read t3f2                              # tek blok / task
pg read stack:10-24                       # satır aralığı
pg related payments/refund.go             # bir kod dosyasının kuralları, kararları, hataları
pg find --type error --since 30d          # metadata filtresi
```

Adresler: `proje/not`, `not`, `not#Başlık`, `not#Üst/Alt`, `not^blok`, `^blok`, `not:10-24`, `@ULID`.

Çıkış kodları: 0 başarılı, 1 sonuç yok, 2 kullanım hatası, 3 adres bulunamadı, 4 çakışma (proje zaten var), 5 böyle bir space yok, 7 iç hata.

## Geliştirme

```sh
go test ./...
go run ./tools/gen -n 500 -out /tmp/bench    # performans için sentetik space (PARAGMA_HOME=/tmp/bench)
PARAGMA_TRACE=1 pg search x                  # aşama süreleri (stderr)
```
