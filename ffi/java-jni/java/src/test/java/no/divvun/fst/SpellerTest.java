package no.divvun.fst;

import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.BeforeAll;
import static org.junit.jupiter.api.Assertions.*;

class SpellerTest {
    private static final String SPELLER_PATH = "../../../se.bhfst";

    @BeforeAll
    static void loadLibrary() {
        String libPath = System.getProperty("divvun.fst.library.path");
        if (libPath != null) {
            System.load(libPath + "/" + getLibraryName());
        } else {
            System.loadLibrary("divvun_fst_jni");
        }
    }

    private static String getLibraryName() {
        String osName = System.getProperty("os.name").toLowerCase();
        if (osName.contains("mac")) {
            return "libdivvun_fst_jni.dylib";
        } else if (osName.contains("linux")) {
            return "libdivvun_fst_jni.so";
        } else if (osName.contains("windows")) {
            return "divvun_fst_jni.dll";
        }
        throw new UnsupportedOperationException("Unsupported platform: " + osName);
    }

    @Test
    void testOpenSpellerArchive() throws Exception {
        try (SpellerArchive archive = SpellerArchive.open(SPELLER_PATH)) {
            assertNotNull(archive);
        }
    }

    @Test
    void testGetSpeller() throws Exception {
        try (SpellerArchive archive = SpellerArchive.open(SPELLER_PATH)) {
            try (Speller speller = archive.getSpeller()) {
                assertNotNull(speller);
            }
        }
    }

    @Test
    void testIsCorrect() throws Exception {
        try (SpellerArchive archive = SpellerArchive.open(SPELLER_PATH);
             Speller speller = archive.getSpeller()) {

            assertTrue(speller.isCorrect("sámegiella"));
            assertFalse(speller.isCorrect("sámegiellla"));
        }
    }

    @Test
    void testSuggest() throws Exception {
        try (SpellerArchive archive = SpellerArchive.open(SPELLER_PATH);
             Speller speller = archive.getSpeller()) {

            try (SuggestionList suggestions = speller.suggest("sámegiellla")) {
                assertNotNull(suggestions);
                assertTrue(suggestions.size() > 0);

                Suggestion first = suggestions.get(0);
                assertNotNull(first.getValue());
                assertTrue(first.getWeight() >= 0);
            }
        }
    }

    @Test
    void testSuggestionListIteration() throws Exception {
        try (SpellerArchive archive = SpellerArchive.open(SPELLER_PATH);
             Speller speller = archive.getSpeller();
             SuggestionList suggestions = speller.suggest("sámegiellla")) {

            int count = 0;
            for (Suggestion s : suggestions) {
                assertNotNull(s.getValue());
                count++;
            }
            assertEquals(suggestions.size(), count);
        }
    }

    @Test
    void testNullInputThrows() throws Exception {
        try (SpellerArchive archive = SpellerArchive.open(SPELLER_PATH);
             Speller speller = archive.getSpeller()) {

            assertThrows(NullPointerException.class, () -> speller.isCorrect(null));
            assertThrows(NullPointerException.class, () -> speller.suggest(null));
        }
    }

    @Test
    void testClosedSpellerThrows() throws Exception {
        Speller speller;
        try (SpellerArchive archive = SpellerArchive.open(SPELLER_PATH)) {
            speller = archive.getSpeller();
        }
        speller.close();

        assertThrows(IllegalStateException.class, () -> speller.isCorrect("test"));
        assertThrows(IllegalStateException.class, () -> speller.suggest("test"));
    }
}
