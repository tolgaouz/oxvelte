<script>
	let node;

	function mutate() {
		node['remove']();
		node['innerHTML'] = 'changed';
	}
</script>

<div bind:this={node}>content</div>
