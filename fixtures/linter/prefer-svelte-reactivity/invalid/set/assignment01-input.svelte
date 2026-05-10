<script lang="ts">
	let selected: Set<string>;

	function reset() {
		selected = new Set<string>();
	}

	function select(id: string) {
		selected.add(id);
	}
</script>
