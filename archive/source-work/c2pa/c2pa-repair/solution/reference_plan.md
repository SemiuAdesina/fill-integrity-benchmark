# Reference repair plan

Sort and deduplicate the supplied legal universe by the public edit encoding.
Enumerate subsets globally, applying each complete subset in exact canonical
order. Run the full update validator only after application. Rank successful
repairs by edit count and then the vector of edit keys. Return the selected
sequence, post-edit ordered manifest, and reference graph computed by validation.
