// Package catalog loads the product catalog shipped in testdata/ and
// production builds, and verifies it against a pinned checksum.
package catalog

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
)

// ExpectedChecksum pins the catalog content (sha256 of the file).
const ExpectedChecksum = "4ff52f25fc9d34a68b3361410dec09a2f557df8aa536fc6d58cc7ff005e37ea5"

// Product is one catalog entry.
type Product struct {
	SKU   string `json:"sku"`
	Name  string `json:"name"`
	Price int64  `json:"price"`
}

// Load reads and verifies the catalog.
func Load(path string) ([]Product, error) {
	b, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	sum := sha256.Sum256(b)
	if got := hex.EncodeToString(sum[:]); got != ExpectedChecksum {
		return nil, fmt.Errorf("catalog checksum mismatch: got %s want %s", got[:12], ExpectedChecksum[:12])
	}
	var ps []Product
	if err := json.Unmarshal(b, &ps); err != nil {
		return nil, err
	}
	return ps, nil
}
