package catalog

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

const pinned = "4ff52f25fc9d34a68b3361410dec09a2f557df8aa536fc6d58cc7ff005e37ea5"

const lf = "[\n  {\"sku\": \"TSHIRT-01\", \"name\": \"T-shirt\", \"price\": 34900},\n  {\"sku\": \"MUG-02\", \"name\": \"Mug\", \"price\": 12900},\n  {\"sku\": \"CAP-03\", \"name\": \"Cap\", \"price\": 19900}\n]\n"

// The pinned checksum must stay: CI on Linux computes it over the LF file.
func TestHidden_ChecksumUnchanged(t *testing.T) {
	if ExpectedChecksum != pinned {
		t.Fatalf("ExpectedChecksum was changed to %s", ExpectedChecksum)
	}
}

// A CRLF checkout (Windows) must load.
func TestHidden_LoadsCRLF(t *testing.T) {
	p := filepath.Join(t.TempDir(), "c.json")
	os.WriteFile(p, []byte(strings.ReplaceAll(lf, "\n", "\r\n")), 0o644)
	if _, err := Load(p); err != nil {
		t.Fatal(err)
	}
}

// An LF checkout (CI) must still load.
func TestHidden_LoadsLF(t *testing.T) {
	p := filepath.Join(t.TempDir(), "c.json")
	os.WriteFile(p, []byte(lf), 0o644)
	ps, err := Load(p)
	if err != nil || len(ps) != 3 {
		t.Fatal(ps, err)
	}
}

// A real edit of the content must still be rejected.
func TestHidden_RejectsTampered(t *testing.T) {
	p := filepath.Join(t.TempDir(), "c.json")
	os.WriteFile(p, []byte(strings.Replace(lf, "34900", "1", 1)), 0o644)
	if _, err := Load(p); err == nil {
		t.Fatal("tampered catalog accepted")
	}
}
