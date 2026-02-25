<script>
	let report = null
	let results = null
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

	function asPercentage(input) {
		const v = input / report.results.length * 100
		return v.toFixed(2)
	}

	function firstPosition() {
		return asPercentage(report.results.filter(r => r.position === 0).length)
	}

	function topFive() {
		return asPercentage(report.results.filter(r => r.position !== null && r.position < 5).length)
	}

	function anywhere() {
		return asPercentage(report.results.filter(r => r.position !== null).length)
	}

	function noSuggestions() {
		return asPercentage(report.results.filter(r => r.suggestions.length === 0).length)
	}

	function onlyWrong() {
		return asPercentage(report.results.filter(r => r.position === null && r.suggestions.length > 0).length)
	}

	function precision() {
		const anywhereCount = report.results.filter(r => r.position !== null).length
		const withSuggestions = report.results.filter(r => r.suggestions.length > 0).length
		if (withSuggestions === 0) return "0.00"
		return ((anywhereCount / withSuggestions) * 100).toFixed(2)
	}

	function recall() {
		const anywhereCount = report.results.filter(r => r.position !== null).length
		return ((anywhereCount / report.results.length) * 100).toFixed(2)
	}

	function accuracy() {
		// Accuracy: correct suggestions / total suggestions (including all wrong ones)
		const correctCount = report.results.filter(r => r.position !== null).length
		const totalSuggestions = report.results.reduce((sum, r) => sum + r.suggestions.length, 0)
		if (totalSuggestions === 0) return "0.00"
		return ((correctCount / totalSuggestions) * 100).toFixed(2)
	}

	function fScore() {
		const p = parseFloat(precision())
		const r = parseFloat(recall())
		if (p + r === 0) return "0.00"
		return ((2 * p * r) / (p + r)).toFixed(2)
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

	function resultClass(result) {
		switch (result.position) {
		case null: {
			if (result.suggestions.length === 0) {
				return "indicator-no-suggestions"
			}
			return "indicator-only-wrong"
		}
		case 0:
		  return "indicator-first"
		case 1:
		  return "indicator-second"
		case 2:
		  return "indicator-third"
		case 3:
		  return "indicator-fourth"
		case 4:
		  return "indicator-fifth"
		default:
		  return "indicator-after-fifth"
		}
	}

	function fetchReport() {
		return fetch(`report.json`)
				.then(r => r.json())
				.then(data => { 
					report = data
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
	padding: 0.5em 1em;
	vertical-align: top;
}
strong {
	font-weight: 600;
}

.table ol {
	margin: 0;
	padding-left: 1.2em;
}
.table ol li {
	margin-top: 0.2em;
	margin-bottom: 0.2em;
}

.table small {
	opacity: 0.8;
	margin-left: 1em;
}
.indicator-only-wrong {
	background-color: #f001;
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
	padding: 0.3em 0.6em;
}
.stats-table td {
	text-align: right;
}
.stats-table th {
	text-align: left;
	background-color: #f5f5f5;
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
	gap: 2em;
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
	min-width: 8em;
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
<table class="stats-table">
	<tr>
		<th>Words: {report.results.length}</th>
	  <th>
			Words per second
		</th>
		<th>
			Total runtime
		</th>
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

<h2>Suggestion Statistics</h2>
<div class="accuracy-stats-container">
	<div>
		<ul>
		<li>
			% in 1st position: {firstPosition()}%
		</li>
		<li>
			% in top 5: {topFive()}%
		</li>
		<li>
			% anywhere: {anywhere()}%
		</li>
		<li>
			% no suggestions: {noSuggestions()}%
		</li>
		<li>
			% only wrong: {onlyWrong()}%
		</li>
		</ul>
		
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
{:else}
<p>Sorted in some unknown way (this is a bug)</p>
{/if}

<button on:click={sortByTime}>Sort by Time</button>
<button on:click={sortByPosition}>Sort by Position</button>
<button on:click={sortByDistance}>Sort by Edit Distance</button>
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
				&rarr;
				<span class="word">{result.expected}</span>
			</p>
			<p>
				<strong>Result:</strong>
			{#if result.position === null}
				Not in suggestions
			{:else if result.position === 0}
				Top suggestion
			{:else}
				Suggestion {result.position + 1}
			{/if}
			</p>
			<p>
				<strong>Edit distance:</strong> {result.distance}
			</p>
			<p>
			<strong>Time:</strong> {humanTimeMillis(result.time)}
			</p>
		</td>
		<td>
		{#if result.suggestions.length > 0}
			<ol>
			{#each result.suggestions as suggestion, i}
				<li>
					<span class={wordClass(result, i)}>
						{suggestion.value}
					</span>
					<small>{suggestion.weight}</small>
				</li>
			{/each}
			</ol>
		{:else}
			No suggestions
		{/if}
		</td>
	</tr>
{/each}
	</tbody>
</table>
{/if}