<script>
	let things = [{ id: 1 }];
</script>

{#each things as thing ('thing')}
	{thing.id}
{/each}
