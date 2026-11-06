package cart

import "testing"

func TestHidden_TotalByCountry(t *testing.T) {
	lines := []Line{{SKU: "a", Price: 1000, Quantity: 2}, {SKU: "b", Price: 500, Quantity: 1}}
	de := Cart{Lines: lines, Country: "DE"}
	if got := de.Total(); got != 2975 {
		t.Errorf("DE total %d, want 2975", got)
	}
	none := Cart{Lines: lines}
	if got := none.Total(); got != 3000 {
		t.Errorf("total without a country %d, want 3000", got)
	}
}
