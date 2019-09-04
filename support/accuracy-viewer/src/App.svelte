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

</style>

{#if report != null}
<p>Speller configuration:</p>
<pre>
{JSON.stringify(report.config, null, 2)}
</pre>

<table>
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
		<th>Real</th>
		<td>{wordsPerSecond(report.total_time)}</td>
		<td>{humanTime(report.total_time)}</td>
	</tr>
	<tr>
		<th>CPU<br><small>(linear "user" time)</small></th>
		<td>{wordsPerSecond(totalRuntime)}</td>
		<td>{humanTime(totalRuntime)}</td>
	</tr>
</table>

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

{#if sortMode == null}
<p>Sorted by input order</p>
{:else if sortMode === "time:asc"}
<p>Sorted by time, ascending</p>
{:else if sortMode === "time:desc"}
<p>Sorted by time, descending</p>
{:else}
<p>Sorted in some unknown way (this is a bug)</p>
{/if}

{/if}

{#if results == null}
Loading
{:else}
<a href="#" on:click={sortByTime}>Sort by Time</a>
<table class="table">
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