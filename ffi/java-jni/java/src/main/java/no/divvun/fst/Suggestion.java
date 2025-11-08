package no.divvun.fst;

public class Suggestion {
    private final String value;
    private final float weight;
    private final Boolean completed;

    Suggestion(String value, float weight, Boolean completed) {
        if (value == null) {
            throw new NullPointerException("value cannot be null");
        }
        this.value = value;
        this.weight = weight;
        this.completed = completed;
    }

    public String getValue() {
        return value;
    }

    public float getWeight() {
        return weight;
    }

    public Boolean getCompleted() {
        return completed;
    }

    @Override
    public String toString() {
        return "Suggestion{value='" + value + "', weight=" + weight +
               ", completed=" + completed + "}";
    }

    @Override
    public boolean equals(Object obj) {
        if (this == obj) return true;
        if (!(obj instanceof Suggestion)) return false;
        Suggestion other = (Suggestion) obj;
        return value.equals(other.value) &&
               Float.compare(weight, other.weight) == 0 &&
               (completed == null ? other.completed == null : completed.equals(other.completed));
    }

    @Override
    public int hashCode() {
        int result = value.hashCode();
        result = 31 * result + Float.floatToIntBits(weight);
        result = 31 * result + (completed != null ? completed.hashCode() : 0);
        return result;
    }
}
