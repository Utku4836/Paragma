package catalog

import "testing"

func TestLoad(t *testing.T) {
	ps, err := Load("testdata/catalog.json")
	if err != nil {
		t.Fatal(err)
	}
	if len(ps) != 3 || ps[0].SKU != "TSHIRT-01" {
		t.Fatalf("unexpected catalog: %+v", ps)
	}
}
