package no.divvun.fst;

public class SpellerConfig {
    private final Integer nBest;
    private final Float maxWeight;
    private final Float beam;
    private final ReweightingConfig reweight;
    private final int nodePoolSize;
    private final boolean recase;
    private final String completionMarker;

    private SpellerConfig(Builder builder) {
        this.nBest = builder.nBest;
        this.maxWeight = builder.maxWeight;
        this.beam = builder.beam;
        this.reweight = builder.reweight;
        this.nodePoolSize = builder.nodePoolSize;
        this.recase = builder.recase;
        this.completionMarker = builder.completionMarker;
    }

    public Integer getNBest() {
        return nBest;
    }

    public Float getMaxWeight() {
        return maxWeight;
    }

    public Float getBeam() {
        return beam;
    }

    public ReweightingConfig getReweight() {
        return reweight;
    }

    public int getNodePoolSize() {
        return nodePoolSize;
    }

    public boolean isRecase() {
        return recase;
    }

    public String getCompletionMarker() {
        return completionMarker;
    }

    public static Builder builder() {
        return new Builder();
    }

    public static class Builder {
        private Integer nBest = 10;
        private Float maxWeight = 10000.0f;
        private Float beam = null;
        private ReweightingConfig reweight = null;
        private int nodePoolSize = 128;
        private boolean recase = true;
        private String completionMarker = null;

        public Builder nBest(Integer nBest) {
            this.nBest = nBest;
            return this;
        }

        public Builder maxWeight(Float maxWeight) {
            this.maxWeight = maxWeight;
            return this;
        }

        public Builder beam(Float beam) {
            this.beam = beam;
            return this;
        }

        public Builder reweight(ReweightingConfig reweight) {
            this.reweight = reweight;
            return this;
        }

        public Builder nodePoolSize(int nodePoolSize) {
            this.nodePoolSize = nodePoolSize;
            return this;
        }

        public Builder recase(boolean recase) {
            this.recase = recase;
            return this;
        }

        public Builder completionMarker(String completionMarker) {
            this.completionMarker = completionMarker;
            return this;
        }

        public SpellerConfig build() {
            return new SpellerConfig(this);
        }
    }

    @Override
    public String toString() {
        return "SpellerConfig{nBest=" + nBest +
               ", maxWeight=" + maxWeight +
               ", beam=" + beam +
               ", reweight=" + reweight +
               ", nodePoolSize=" + nodePoolSize +
               ", recase=" + recase +
               ", completionMarker='" + completionMarker + "'" +
               "}";
    }
}
