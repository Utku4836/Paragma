// Package payments charges customers through the payment provider.
package payments

import (
	"context"
	"errors"
	"fmt"
	"time"

	"example.com/shopkit/audit"
	"example.com/shopkit/clock"
)

// Payment is a completed charge.
type Payment struct {
	ID       string
	Amount   int64 // kuruş
	At       time.Time
	Refunded int64 // kuruş already refunded
}

// ErrInvalidAmount is returned for non-positive amounts.
var ErrInvalidAmount = errors.New("payments: invalid amount")

// Provider is the payment gateway.
type Provider interface {
	Charge(ctx context.Context, amount int64) (string, error)
	Refund(ctx context.Context, paymentID string, amount int64) error
}

// Charge takes money from the customer and records it.
func Charge(ctx context.Context, p Provider, amount int64) (*Payment, error) {
	if amount <= 0 {
		return nil, ErrInvalidAmount
	}
	id, err := p.Charge(ctx, amount)
	if err != nil {
		return nil, fmt.Errorf("charge: %w", err)
	}
	audit.Record(ctx, "charge", id)
	return &Payment{ID: id, Amount: amount, At: clock.Now()}, nil
}
