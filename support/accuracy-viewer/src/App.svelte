<script>
	let report = null
	let results = null
	let originalResults = null
	let sortMode = null
	$: totalRuntime = calculateTotalRuntime(report)

	function sortByTime() {
		const sorter = (a, b) => {
			if (a.time.secs === b.time.secs) {
				if (a.time.subsec_nanos === b.time.subsec_nanos) {
					return 0
				}
				return a.time.subsec_nanos > b.time.subsec_nanos ? -1 : 1
			} else {
				return a.time.secs > b.time.secs ? -1 : 1
			}
		}

		if (sortMode === "time:asc") {
			results.reverse()
			sortMode = "time:desc"
		} else {
			results.sort(sorter)
			sortMode = "time:asc"
		}
		
		results = results
	}

	function getPositionSortValue(result) {
		// No suggestions is worst
		if (result.position === null && result.suggestions.length === 0) {
			return Number.MAX_SAFE_INTEGER
		}
		// Only wrong suggestions is second worst
		if (result.position === null && result.suggestions.length > 0) {
			return Number.MAX_SAFE_INTEGER - 1
		}
		// Actual positions
		return result.position
	}

	function sortByPosition() {
		const sorter = (a, b) => {
			const aVal = getPositionSortValue(a)
			const bVal = getPositionSortValue(b)
			return aVal - bVal
		}

		if (sortMode === "position:asc") {
			results.reverse()
			sortMode = "position:desc"
		} else {
			results.sort(sorter)
			sortMode = "position:asc"
		}
		
		results = results
	}

	function sortByDistance() {
		const sorter = (a, b) => {
			return a.distance - b.distance
		}

		if (sortMode === "distance:asc") {
			results.reverse()
			sortMode = "distance:desc"
		} else {
			results.sort(sorter)
			sortMode = "distance:asc"
		}
		
		results = results
	}

	function sortByInputOrder() {
		results = originalResults.slice()
		sortMode = null
	}

	function sortByClassification() {
		const classificationOrder = { 'TP': 0, 'TN': 1, 'FP': 2, 'FN': 3 };
		
		const sorter = (a, b) => {
			const aType = getClassificationType(a);
			const bType = getClassificationType(b);
			return classificationOrder[aType] - classificationOrder[bType];
		}

		if (sortMode === "classification:asc") {
			results.reverse()
			sortMode = "classification:desc"
		} else {
			results.sort(sorter)
			sortMode = "classification:asc"
		}
		
		results = results
	}

	function asPercentage(input) {
		const v = input / report.results.length * 100
		return v.toFixed(2)
	}

	// Get only True Positive words (error words classified as incorrect)
	function getTruePositives() {
		return report.results.filter(r => {
			const isPair = r.expected !== null;
			if (isPair) {
				// Error word: TP if not false_accept (false_accept must be explicitly true)
				return r.false_accept !== true;
			}
			// Correct word: never TP
			return false;
		});
	}

	function firstPosition() {
		const tpWords = getTruePositives();
		if (tpWords.length === 0) return "0.00";
		const count = tpWords.filter(r => r.position === 0).length;
		return ((count / tpWords.length) * 100).toFixed(2);
	}

	function topFive() {
		const tpWords = getTruePositives();
		if (tpWords.length === 0) return "0.00";
		const count = tpWords.filter(r => r.position !== null && r.position < 5).length;
		return ((count / tpWords.length) * 100).toFixed(2);
	}

	function anywhere() {
		const tpWords = getTruePositives();
		if (tpWords.length === 0) return "0.00";
		const count = tpWords.filter(r => r.position !== null).length;
		return ((count / tpWords.length) * 100).toFixed(2);
	}

	function noSuggestions() {
		const tpWords = getTruePositives();
		if (tpWords.length === 0) return "0.00";
		const count = tpWords.filter(r => r.suggestions.length === 0).length;
		return ((count / tpWords.length) * 100).toFixed(2);
	}

	function onlyWrong() {
		const tpWords = getTruePositives();
		if (tpWords.length === 0) return "0.00";
		const count = tpWords.filter(r => r.position === null && r.suggestions.length > 0).length;
		return ((count / tpWords.length) * 100).toFixed(2);
	}

	function firstPositionCount() {
		return getTruePositives().filter(r => r.position === 0).length;
	}

	function topFiveCount() {
		return getTruePositives().filter(r => r.position !== null && r.position < 5).length;
	}

	function anywhereCount() {
		return getTruePositives().filter(r => r.position !== null).length;
	}

	function noSuggestionsCount() {
		return getTruePositives().filter(r => r.suggestions.length === 0).length;
	}

	function onlyWrongCount() {
		return getTruePositives().filter(r => r.position === null && r.suggestions.length > 0).length;
	}

	function precision() {
		const tpWords = getTruePositives();
		const anywhereCount = tpWords.filter(r => r.position !== null).length;
		const withSuggestions = tpWords.filter(r => r.suggestions.length > 0).length;
		if (withSuggestions === 0) return "0.00"
		return ((anywhereCount / withSuggestions) * 100).toFixed(2)
	}

	function recall() {
		const tpWords = getTruePositives();
		if (tpWords.length === 0) return "0.00";
		const anywhereCount = tpWords.filter(r => r.position !== null).length;
		return ((anywhereCount / tpWords.length) * 100).toFixed(2);
	}

	function accuracy() {
		// Accuracy: correct suggestions / total suggestions (including all wrong ones)
		const tpWords = getTruePositives();
		const correctCount = tpWords.filter(r => r.position !== null).length;
		const totalSuggestions = tpWords.reduce((sum, r) => sum + r.suggestions.length, 0);
		if (totalSuggestions === 0) return "0.00";
		return ((correctCount / totalSuggestions) * 100).toFixed(2);
	}

	function fScore() {
		const p = parseFloat(precision())
		const r = parseFloat(recall())
		if (p + r === 0) return "0.00"
		return ((2 * p * r) / (p + r)).toFixed(2)
	}

	// Spell checker classification metrics (based on accept/reject behavior)
	function classifierPrecision() {
		const tp = report.summary.true_positive || 0
		const fp = report.summary.false_accept || 0
		if (tp + fp === 0) return "N/A"
		return ((tp / (tp + fp)) * 100).toFixed(2)
	}

	function classifierRecall() {
		const tp = report.summary.true_positive || 0
		const fn = report.summary.false_negative || 0
		if (tp + fn === 0) return "N/A"
		return ((tp / (tp + fn)) * 100).toFixed(2)
	}

	function classifierAccuracy() {
		const tp = report.summary.true_positive || 0
		const tn = report.summary.true_negative || 0
		const fp = report.summary.false_accept || 0
		const fn = report.summary.false_negative || 0
		const total = tp + tn + fp + fn
		if (total === 0) return "N/A"
		return (((tp + tn) / total) * 100).toFixed(2)
	}

	function classifierFScore() {
		const p = classifierPrecision()
		const r = classifierRecall()
		if (p === "N/A" || r === "N/A") return "N/A"
		const pNum = parseFloat(p)
		const rNum = parseFloat(r)
		if (pNum + rNum === 0) return "0.00"
		return ((2 * pNum * rNum) / (pNum + rNum)).toFixed(2)
	}

	function formatMetric(value) {
		return value === "N/A" ? value : value + "%"
	}

	function humanTimeMillis(time) {
		const ms = time.secs * 1000 + time.subsec_nanos / 1000000
		return `${ms} ms`
	}

	function humanTime(time) {
		let s = timeToFloat(time)
		if (s > 60) {
			const m = Math.floor(s / 60)
			s = s % 60
			return `${m}:${s.toFixed(3)}`
		}

		return `00:${s.toFixed(3)}`
	}

	function timeToFloat(time) {
		return time.secs + time.subsec_nanos / 1e12
	}

	function calculateTotalRuntime(report) {
		if (report == null) {
			return { secs: 0, subsec_nanos: 0 }
		}
		const count = report.results.reduce(
			(acc, cur) => timeToFloat(cur.time) + acc,
			0.0)
		const [secs, subsec_nanos] = count.toString().split(".")
		return { secs: parseInt(secs, 10), subsec_nanos: parseInt(subsec_nanos, 10) }
	}
	
	function wordsPerSecond(totalRuntime) {
		const len = report.results.length
		const total = timeToFloat(totalRuntime)
		console.log(totalRuntime, len, total)
		return (len / total).toFixed(2)
	}
	
	function wordClass(result, i) {
		if (result.position === i) {
			return "word word-correct"
		}

		return "word"
	}

	function getClassificationType(result) {
		const isPair = result.expected !== null;  // Is this a test pair (error word)?
		
		if (isPair) {
			// Input is an error word (expected has a correction)
			// For error words, false_accept determines if spellchecker accepts or flags it
			if (!result.false_accept) {
				return 'TP'; // True Positive: error word flagged as incorrect
			} else {
				return 'FN'; // False Negative: error word incorrectly accepted
			}
		} else {
			// Input is a correct word (expected is null)
			// Use false_accept flag to determine classification
			if (result.false_accept) {
				return 'FP'; // False Positive: correct word incorrectly flagged
			} else {
				return 'TN'; // True Negative: correct word correctly accepted
			}
		}
	}
	
	function getClassificationLabel(result) {
		const type = getClassificationType(result);
		switch(type) {
			case 'TP': return 'True positive';
			case 'FN': return 'False negative';
			case 'TN': return 'True negative';
			case 'FP': return 'False positive';
			default: return '';
		}
	}

	function resultClass(result) {
		const classType = getClassificationType(result);
		
		if (classType === 'TP') {
			// True Positive: error word correctly classified as error
			// Show different shades based on whether correction was found
			if (result.position === 0) {
				return "indicator-tp-first"; // Correction in first position
			} else if (result.position !== null) {
				return "indicator-tp-found"; // Correction found elsewhere
			} else {
				return "indicator-true-positive"; // Classified correctly, correction not in suggestions
			}
		} else if (classType === 'FN') {
			return "indicator-false-negative"; // Error word incorrectly classified as correct (false accept)
		} else if (classType === 'TN') {
			return "indicator-true-negative"; // Correct word correctly classified as correct
		} else if (classType === 'FP') {
			return "indicator-false-positive"; // Correct word incorrectly classified as error
		}
		
		return "indicator-default";
	}

	function fetchReport() {
		return fetch(`report.json`)
				.then(r => r.json())
				.then(data => { 
					report = data
					originalResults = report.results.slice()
					results = report.results.slice()
				})
	}

	function getSpellerTitle(report) {
		if (!report || !report.metadata || !report.metadata.info) {
			return "Spellchecker Accuracy Report"
		}
		const locale = report.metadata.info.locale || "?"
		const title = report.metadata.info.title && report.metadata.info.title[0] 
			? report.metadata.info.title[0].$value || "Spellchecker"
			: "Spellchecker"
		return `${title} (${locale})`
	}

	fetchReport()
</script>

<style>
.table {
	table-layout: fixed;
	border-collapse: collapse;
	width: 100%;
}

.table td, .table th {
  border: 1px solid #cecece;
	padding: 0.3em 0.6em;
	vertical-align: top;
}

.table p {
	margin: 0.3em 0;
}

strong {
	font-weight: 600;
}

.table ol {
	margin: 0.6em 0 0 0;
	padding-left: 1.2em;
}

.table td > em {
	display: block;
	margin-top: 0.6em;
}

.table ol li {
	margin-top: 0.2em;
	margin-bottom: 0.2em;
}

.table small {
	opacity: 0.8;
	margin-left: 1em;
}
.weight-details {
	display: inline-block;
	color: #666;
	font-size: 0.9em;
}
/* Classification-based indicators */
.indicator-true-positive {
	background-color: #ff02;  /* Light orange - TP but correction not in suggestions */
}
.indicator-tp-first {
	background-color: #0f04;  /* Light green - TP with correction in first position */
}
.indicator-tp-found {
	background-color: #6f03;  /* Light olive - TP with correction found elsewhere */
}
.indicator-false-negative {
	background-color: #f006;  /* Strong red - error word incorrectly classified as correct */
}
.indicator-true-negative {
	background-color: #0f06;  /* Strong green - correct word correctly classified */
}
.indicator-false-positive {
	background-color: #f006;  /* Strong red - correct word incorrectly classified as error */
}
.indicator-default {
	background-color: #ccc3;  /* Gray - fallback */
}
/* Old indicators for backward compatibility */
.indicator-only-wrong {
	background-color: #f001;
}
.indicator-false-accept {
	background-color: #f601;
}
.indicator-tn-first {
	background-color: #0f04;
}
.indicator-tn-found {
	background-color: #6f02;
}
.indicator-tn-not-found {
	background-color: #ff02;
}
.indicator-tn-no-sugg {
	background-color: #ccc3;
}
.indicator-first {
	background-color: #0f02;
}
.indicator-second {
	background-color: #3f01;
}
.indicator-third {
	background-color: #6f01;
}
.indicator-fourth {
	background-color: #9f01;
}
.indicator-fifth {
	background-color: #cf01;
}
.indicator-after-fifth {
	background-color: #fc01;
}
.indicator-no-suggestions {
	background-color: #f901;
}
.word {
	display: inline-block;
	padding: 0.2em;
	border: 1px solid #ccc;
	background-color: #fffa;
	border-radius: 4px;
}
.word.word-correct {
	background-color: #0f04;
	border-color: #4d4;
}
.right {
	text-align: right;
}
.stats-table {
	border-collapse: collapse;
	margin: 0.5em 0;
	font-size: 0.9em;
}
.stats-table th, .stats-table td {
	border: 1px solid #cecece;
	padding: 0.25em 0.4em;
}
.stats-table td {
	text-align: right;
}
.stats-table th {
	text-align: left;
	background-color: #f5f5f5;
	max-width: 150px;
	word-wrap: break-word;
}
button {
	background-color: #4a90e2;
	color: white;
	border: none;
	padding: 0.5em 1em;
	border-radius: 4px;
	cursor: pointer;
	font-size: 1em;
	margin: 0.5em 0.25em;
}
button:hover {
	background-color: #357abd;
}
h1 {
	margin-top: 1em;
	font-size: 2em;
	color: #333;
}
h2 {
	margin-top: 1.5em;
	font-size: 1.3em;
	color: #555;
}
.config-block {
	background-color: #f5f5f5;
	border: 1px solid #ddd;
	border-radius: 4px;
	padding: 0.7em;
	margin: 1em 0;
	overflow-x: auto;
}
.config-block pre {
	margin: 0;
	font-family: 'Monaco', 'Menlo', 'Consolas', monospace;
	font-size: 0.65em;
	line-height: 1.3;
}
.accuracy-stats-container {
	display: flex;
	gap: 1em;
	align-items: flex-start;
}
.accuracy-stats-container > * {
	flex: 1;
}
.metrics-box ul {
	list-style: none;
	padding: 0;
	margin: 0;
}
.metrics-box li {
	margin-bottom: 0.8em;
}
.metrics-box li strong {
	display: inline-block;
	min-width: 6em;
	font-size: 1.05em;
}
.metrics-box li small {
	display: block;
	color: #666;
	font-size: 0.85em;
	margin-top: 0.2em;
	margin-left: 0;
	opacity: 1;
}

</style>

{#if report != null}
<h1>{getSpellerTitle(report)} - Accuracy Report</h1>

<h2>Speller Configuration</h2>
<div class="config-block">
<pre>{JSON.stringify(report.config, null, 2)}</pre>
</div>

<h2>Performance Statistics</h2>
<div class="accuracy-stats-container">
	<div>
		<h3>Runtime</h3>
		<table class="stats-table">
			<tr>
				<th></th>
				<th>Words per second</th>
				<th>Total runtime</th>
			</tr>
			<tr>
				<th>Real<br><small>(clock time, parallelised processing)</small></th>
				<td>{wordsPerSecond(report.total_time)}</td>
				<td>{humanTime(report.total_time)}</td>
			</tr>
			<tr>
				<th>CPU<br><small>(estimated serial processing time)</small></th>
				<td>{wordsPerSecond(totalRuntime)}</td>
				<td>{humanTime(totalRuntime)}</td>
			</tr>
			<tr>
				<th>Average per word</th>
				<td>-</td>
				<td>{humanTimeMillis(report.summary.average_time)}</td>
			</tr>
			<tr>
				<th>Average per word (95%)<br><small>(excluding slowest 5%)</small></th>
				<td>-</td>
				<td>{humanTimeMillis(report.summary.average_time_95pc)}</td>
			</tr>
		</table>
	</div>
	<div>
		<h3>Spell Checker Classification</h3>
		<div class="accuracy-stats-container">
			<table class="stats-table">
				<tr>
					<th>True positive<br><small>(correctly flagged)</small></th>
					<td>{report.summary.true_positive || 0}</td>
					<td>{report.results.length > 0 ? ((report.summary.true_positive || 0) / report.results.length * 100).toFixed(1) + '%' : 'N/A'}</td>
				</tr>
				<tr>
					<th>False negative<br><small>(incorrectly accepted)</small></th>
					<td>{report.summary.false_negative || 0}</td>
					<td>{report.results.length > 0 ? ((report.summary.false_negative || 0) / report.results.length * 100).toFixed(1) + '%' : 'N/A'}</td>
				</tr>
				<tr>
					<th>True negative<br><small>(correctly accepted)</small></th>
					<td>{report.summary.true_negative || 0}</td>
					<td>{report.results.length > 0 ? ((report.summary.true_negative || 0) / report.results.length * 100).toFixed(1) + '%' : 'N/A'}</td>
				</tr>
				<tr>
					<th>False positive<br><small>(incorrectly flagged)</small></th>
					<td>{report.summary.false_accept || 0}</td>
					<td>{report.results.length > 0 ? ((report.summary.false_accept || 0) / report.results.length * 100).toFixed(1) + '%' : 'N/A'}</td>
				</tr>
				<tr>
					<th>Total words</th>
					<td>{report.results.length}</td>
					<td>100%</td>
				</tr>
			</table>
			<div class="metrics-box">
				<ul>
					<li>
						<strong>Precision:</strong> {formatMetric(classifierPrecision())}
						<small>Of words flagged as incorrect, how many are actually incorrect</small>
					</li>
					<li>
						<strong>Recall:</strong> {formatMetric(classifierRecall())}
						<small>Of words that are actually incorrect, how many were flagged as incorrect</small>
					</li>
					<li>
						<strong>Accuracy:</strong> {formatMetric(classifierAccuracy())}
						<small>Correct classifications (TP+TN) out of all words</small>
					</li>
					<li>
						<strong>F-score:</strong> {formatMetric(classifierFScore())}
						<small>Harmonic mean of precision and recall</small>
					</li>
				</ul>
			</div>
		</div>
	</div>
</div>

<h2>Suggestion Statistics</h2>
<p><em>These statistics apply only to true positive words ({report.summary.true_positive || 0} words).</em></p>
<div class="accuracy-stats-container">
	<div>
		<table class="stats-table">
			<tr>
				<th>In 1st position</th>
				<td>{firstPositionCount()}</td>
				<td>{firstPosition()}%</td>
			</tr>
			<tr>
				<th>In top 5</th>
				<td>{topFiveCount()}</td>
				<td>{topFive()}%</td>
			</tr>
			<tr>
				<th>Anywhere</th>
				<td>{anywhereCount()}</td>
				<td>{anywhere()}%</td>
			</tr>
			<tr>
				<th>No suggestions</th>
				<td>{noSuggestionsCount()}</td>
				<td>{noSuggestions()}%</td>
			</tr>
			<tr>
				<th>Only wrong</th>
				<td>{onlyWrongCount()}</td>
				<td>{onlyWrong()}%</td>
			</tr>
		</table>
		
		<ul>
		{#if report.summary && report.summary.average_position_of_correct !== undefined}
		<li>
			Average position of correct: {report.summary.average_position_of_correct.toFixed(2)}
		</li>
		{/if}
		{#if report.summary && report.summary.average_suggestions_for_correct !== undefined}
		<li>
			Average suggestions for correct: {report.summary.average_suggestions_for_correct.toFixed(2)}
		</li>
		{/if}
		</ul>
	</div>
	<div class="metrics-box">
		<ul>
			<li>
				<strong>Precision:</strong> {precision()}%
				<small>Of words that got suggestions, how many got the correct one</small>
			</li>
			<li>
				<strong>Recall:</strong> {recall()}%
				<small>Of all misspelled words, how many got the correct suggestion</small>
			</li>
			<li>
				<strong>Accuracy:</strong> {accuracy()}%
				<small>Correct suggestions out of all suggestions (indicates noise level)</small>
			</li>
			<li>
				<strong>F-score:</strong> {fScore()}%
				<small>Harmonic mean of precision and recall; high only when both are good</small>
			</li>
		</ul>
	</div>
</div>

{/if}

{#if results == null}
Loading
{:else}
<h2>Detailed Results</h2>

{#if sortMode == null}
<p>Sorted by input order</p>
{:else if sortMode === "time:asc"}
<p>Sorted by time, ascending (slowest first)</p>
{:else if sortMode === "time:desc"}
<p>Sorted by time, descending (fastest first)</p>
{:else if sortMode === "position:asc"}
<p>Sorted by position, ascending (best first)</p>
{:else if sortMode === "position:desc"}
<p>Sorted by position, descending (worst first)</p>
{:else if sortMode === "distance:asc"}
<p>Sorted by edit distance, ascending (smallest first)</p>
{:else if sortMode === "distance:desc"}
<p>Sorted by edit distance, descending (largest first)</p>
{:else if sortMode === "classification:asc"}
<p>Sorted by classification (TP → TN → FP → FN)</p>
{:else if sortMode === "classification:desc"}
<p>Sorted by classification (FN → FP → TN → TP)</p>
{:else}
<p>Sorted in some unknown way (this is a bug)</p>
{/if}

<button on:click={sortByInputOrder}>Sort by Input Order</button>
<button on:click={sortByTime}>Sort by Time</button>
<button on:click={sortByPosition}>Sort by Position</button>
<button on:click={sortByDistance}>Sort by Edit Distance</button>
<button on:click={sortByClassification}>Sort by Classification</button>
<table class="table">
	<thead>
		<tr>
			<th>Spelling error data</th>
			<th>Suggestion list</th>
		</tr>
	</thead>
	<tbody>
{#each results as result}
	<tr class={resultClass(result)} id="{result.input}">
		<td class="right">
			<p>
				<a href="#{result.input}" class="word">{result.input}</a>
				{#if result.expected !== null}
					&rarr;
					<span class="word">{result.expected}</span>
				{/if}
			</p>
			<p>
				<strong>Result:</strong>
				<span class="classification-label" style="font-weight: bold; color: {getClassificationType(result) === 'FP' || getClassificationType(result) === 'FN' ? '#d00' : '#080'};">
					{getClassificationLabel(result)}
				</span>
			{#if getClassificationType(result) === 'TP'}
				{#if result.position === null}
					<br><small>Not in suggestions</small>
				{:else if result.position === 0}
					<br><small>Top suggestion</small>
				{:else}
					<br><small>Suggestion {result.position + 1}</small>
				{/if}
			{/if}
			</p>
			{#if getClassificationType(result) === 'TP' || getClassificationType(result) === 'FN'}
			<p>
				<strong>Edit distance:</strong> {result.distance}
			</p>
			{/if}
			{#if getClassificationType(result) === 'TP'}
			<p>
			<strong>Time:</strong> {humanTimeMillis(result.time)}
			</p>
			{/if}
		</td>
		<td>
		{#if result.false_accept && getClassificationType(result) === 'FN'}
			<em>Incorrectly accepted as correct</em>
		{:else if result.suggestions.length > 0}
			<ol>
			{#each result.suggestions as suggestion, i}
				<li>
					<span class={wordClass(result, i)}>
						{suggestion.value}
					</span>
					<small>
						{suggestion.weight.toFixed(5)}
						{#if suggestion.weight_details}
							<span class="weight-details">
								(lex: {suggestion.weight_details.lexicon_weight.toFixed(5)}, 
								mut: {suggestion.weight_details.mutator_weight.toFixed(5)}, 
								rew: {suggestion.weight_details.reweight_start.toFixed(0)}/{suggestion.weight_details.reweight_mid < 0 ? '-' : suggestion.weight_details.reweight_mid.toFixed(0)}/{suggestion.weight_details.reweight_end.toFixed(0)})
							</span>
						{/if}
					</small>
				</li>
			{/each}
			</ol>
		{:else if getClassificationType(result) !== 'TN'}
			<em>No suggestions</em>
		{/if}
		</td>
	</tr>
{/each}
	</tbody>
</table>
{/if}