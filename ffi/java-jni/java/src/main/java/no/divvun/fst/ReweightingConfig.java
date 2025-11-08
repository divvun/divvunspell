package no.divvun.fst;

public class ReweightingConfig {
    private final float startPenalty;
    private final float endPenalty;
    private final float midPenalty;

    public ReweightingConfig(float startPenalty, float endPenalty, float midPenalty) {
        this.startPenalty = startPenalty;
        this.endPenalty = endPenalty;
        this.midPenalty = midPenalty;
    }

    public ReweightingConfig() {
        this(10.0f, 10.0f, 5.0f);
    }

    public float getStartPenalty() {
        return startPenalty;
    }

    public float getEndPenalty() {
        return endPenalty;
    }

    public float getMidPenalty() {
        return midPenalty;
    }

    @Override
    public String toString() {
        return "ReweightingConfig{startPenalty=" + startPenalty +
               ", endPenalty=" + endPenalty +
               ", midPenalty=" + midPenalty + "}";
    }
}
