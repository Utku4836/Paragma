package payments

import (
	"context"
	"testing"

	"example.com/shopkit/audit"
)

type fakeProvider struct{ refunds []int64 }

func (f *fakeProvider) Charge(ctx context.Context, amount int64) (string, error) { return "pay_1", nil }
func (f *fakeProvider) Refund(ctx context.Context, id string, amount int64) error {
	f.refunds = append(f.refunds, amount)
	return nil
}

func TestCharge(t *testing.T) {
	audit.Reset()
	p, err := Charge(context.Background(), &fakeProvider{}, 1500)
	if err != nil || p.Amount != 1500 {
		t.Fatal(p, err)
	}
	if e := audit.Entries(); len(e) != 1 || e[0].Action != "charge" {
		t.Fatal(e)
	}
	if _, err := Charge(context.Background(), &fakeProvider{}, 0); err != ErrInvalidAmount {
		t.Fatal(err)
	}
}
