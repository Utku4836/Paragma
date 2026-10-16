package payments

import (
	"context"
	"errors"
	"testing"
	"time"

	"example.com/shopkit/audit"
	"example.com/shopkit/clock"
)

var pinnedNow = time.Date(2026, 1, 15, 12, 0, 0, 0, time.UTC)

type hiddenProvider struct{ refunds []int64 }

func (h *hiddenProvider) Charge(ctx context.Context, amount int64) (string, error) { return "pay_h", nil }
func (h *hiddenProvider) Refund(ctx context.Context, id string, amount int64) error {
	h.refunds = append(h.refunds, amount)
	return nil
}

type failingProvider struct{}

func (failingProvider) Charge(ctx context.Context, amount int64) (string, error) { return "", nil }
func (failingProvider) Refund(ctx context.Context, id string, amount int64) error {
	return errors.New("gateway down")
}

func pin(t *testing.T) {
	old := clock.Now
	clock.Now = func() time.Time { return pinnedNow }
	t.Cleanup(func() { clock.Now = old })
}

// Within 30 days a partial refund goes through, is recorded and audited.
// The clock is pinned, so code calling time.Now() instead of clock.Now()
// sees the payment as months old and fails here.
func TestHidden_PartialRefundInWindow(t *testing.T) {
	pin(t)
	audit.Reset()
	pr := &hiddenProvider{}
	pay := &Payment{ID: "pay_9", Amount: 1000, At: pinnedNow.Add(-29 * 24 * time.Hour)}
	if err := Refund(context.Background(), pr, pay, 300); err != nil {
		t.Fatal(err)
	}
	if len(pr.refunds) != 1 || pr.refunds[0] != 300 || pay.Refunded != 300 {
		t.Fatalf("refunds %v, Refunded %d", pr.refunds, pay.Refunded)
	}
	found := false
	for _, e := range audit.Entries() {
		if e.Action == "refund" && e.Ref == "pay_9" {
			found = true
		}
	}
	if !found {
		t.Fatalf("no refund audit entry: %+v", audit.Entries())
	}
}

// Refunds may never exceed what was paid.
func TestHidden_NoOverRefund(t *testing.T) {
	pin(t)
	pr := &hiddenProvider{}
	pay := &Payment{ID: "pay_9", Amount: 1000, At: pinnedNow.Add(-24 * time.Hour), Refunded: 300}
	if err := Refund(context.Background(), pr, pay, 800); err == nil {
		t.Fatal("refund above the paid amount accepted")
	}
	if len(pr.refunds) != 0 {
		t.Fatal("provider called for an invalid refund")
	}
}

// Older than 30 days: refused before calling the provider.
func TestHidden_WindowRefused(t *testing.T) {
	pin(t)
	pr := &hiddenProvider{}
	pay := &Payment{ID: "pay_9", Amount: 1000, At: pinnedNow.Add(-31 * 24 * time.Hour)}
	if err := Refund(context.Background(), pr, pay, 100); err == nil {
		t.Fatal("refund after 30 days accepted")
	}
	if len(pr.refunds) != 0 {
		t.Fatal("provider called outside the window")
	}
}

// Provider errors are wrapped, not swallowed or replaced.
func TestHidden_ProviderErrorWrapped(t *testing.T) {
	pin(t)
	pay := &Payment{ID: "pay_9", Amount: 1000, At: pinnedNow.Add(-24 * time.Hour)}
	err := Refund(context.Background(), failingProvider{}, pay, 100)
	if err == nil || errors.Unwrap(err) == nil {
		t.Fatalf("provider error not wrapped: %v", err)
	}
}

func TestHidden_InvalidAmount(t *testing.T) {
	pin(t)
	pay := &Payment{ID: "pay_9", Amount: 1000, At: pinnedNow}
	if err := Refund(context.Background(), &hiddenProvider{}, pay, 0); err == nil {
		t.Fatal("zero refund accepted")
	}
}
