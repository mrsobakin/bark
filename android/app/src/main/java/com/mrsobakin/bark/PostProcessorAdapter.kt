package com.mrsobakin.bark

import android.annotation.SuppressLint
import android.view.LayoutInflater
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.widget.ImageButton
import android.widget.TextView
import androidx.core.view.ViewCompat
import androidx.recyclerview.widget.RecyclerView
import com.google.android.material.card.MaterialCardView

class PostProcessorAdapter(
    private val onEdit: (Int, PostProcessorStep.Regex) -> Unit,
    private val onRemove: (Int) -> Unit,
    private val onDragStarted: (ViewHolder) -> Unit,
    private val onChanged: (Boolean) -> Unit,
) : RecyclerView.Adapter<PostProcessorAdapter.ViewHolder>() {

    private val steps = mutableListOf<PostProcessorStep>()

    fun replaceAll(newSteps: List<PostProcessorStep>) {
        val previousSize = steps.size
        steps.clear()
        if (previousSize > 0) notifyItemRangeRemoved(0, previousSize)
        steps.addAll(newSteps)
        if (steps.isNotEmpty()) notifyItemRangeInserted(0, steps.size)
        notifyChanged()
    }

    fun snapshot(): List<PostProcessorStep> = steps.toList()

    fun add(step: PostProcessorStep) {
        steps.add(step)
        notifyItemInserted(steps.lastIndex)
        notifyItemRangeChanged(0, steps.size)
        notifyChanged()
    }

    fun update(position: Int, step: PostProcessorStep) {
        if (position !in steps.indices) return
        steps[position] = step
        notifyItemChanged(position)
        notifyChanged()
    }

    fun remove(position: Int): PostProcessorStep? {
        if (position !in steps.indices) return null
        val removed = steps.removeAt(position)
        notifyItemRemoved(position)
        notifyItemRangeChanged(0, steps.size)
        notifyChanged()
        return removed
    }

    fun insert(position: Int, step: PostProcessorStep) {
        val target = position.coerceIn(0, steps.size)
        steps.add(target, step)
        notifyItemInserted(target)
        notifyItemRangeChanged(0, steps.size)
        notifyChanged()
    }

    fun move(from: Int, to: Int): Boolean {
        if (from !in steps.indices || to !in steps.indices || from == to) return false
        val step = steps.removeAt(from)
        steps.add(to, step)
        notifyItemMoved(from, to)
        return true
    }

    fun finishMove() {
        notifyItemRangeChanged(0, steps.size)
        notifyChanged()
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.item_postprocessor, parent, false)
        return ViewHolder(view)
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun onBindViewHolder(holder: ViewHolder, position: Int) {
        val step = steps[position]
        val context = holder.itemView.context

        holder.title.text = when (step) {
            PostProcessorStep.Normalize ->
                context.getString(R.string.postprocessor_title, position + 1, context.getString(R.string.normalize))
            is PostProcessorStep.Regex ->
                context.getString(R.string.postprocessor_title, position + 1, context.getString(R.string.regex_replace))
        }
        holder.summary.text = when (step) {
            PostProcessorStep.Normalize -> context.getString(R.string.normalize_desc)
            is PostProcessorStep.Regex -> context.getString(
                R.string.regex_summary,
                step.pattern.ifEmpty { context.getString(R.string.empty_pattern) },
                step.replacement,
            )
        }

        holder.card.isClickable = step is PostProcessorStep.Regex
        holder.card.isFocusable = step is PostProcessorStep.Regex || steps.size > 1
        holder.edit.visibility = if (step is PostProcessorStep.Regex) View.VISIBLE else View.GONE
        holder.edit.setOnClickListener {
            val currentPosition = holder.bindingAdapterPosition
            val current = steps.getOrNull(currentPosition)
            if (current is PostProcessorStep.Regex) onEdit(currentPosition, current)
        }
        holder.card.setOnClickListener {
            val currentPosition = holder.bindingAdapterPosition
            val current = steps.getOrNull(currentPosition)
            if (current is PostProcessorStep.Regex) onEdit(currentPosition, current)
        }
        holder.remove.setOnClickListener {
            val currentPosition = holder.bindingAdapterPosition
            if (currentPosition != RecyclerView.NO_POSITION) onRemove(currentPosition)
        }
        holder.dragHandle.setOnTouchListener { _, event ->
            if (event.actionMasked == MotionEvent.ACTION_DOWN) onDragStarted(holder)
            false
        }

        holder.clearMoveActions()
        if (position > 0) {
            holder.moveEarlierAction = ViewCompat.addAccessibilityAction(
                holder.card,
                context.getString(R.string.move_step_earlier),
            ) { _, _ ->
                val currentPosition = holder.bindingAdapterPosition
                val moved = currentPosition > 0 && move(currentPosition, currentPosition - 1)
                if (moved) finishMove()
                moved
            }
        }
        if (position < steps.lastIndex) {
            holder.moveLaterAction = ViewCompat.addAccessibilityAction(
                holder.card,
                context.getString(R.string.move_step_later),
            ) { _, _ ->
                val currentPosition = holder.bindingAdapterPosition
                val moved = currentPosition in 0..<steps.lastIndex &&
                    move(currentPosition, currentPosition + 1)
                if (moved) finishMove()
                moved
            }
        }
    }

    override fun getItemCount(): Int = steps.size

    private fun notifyChanged() {
        onChanged(steps.isEmpty())
    }

    class ViewHolder(view: View) : RecyclerView.ViewHolder(view) {
        val card: MaterialCardView = view.findViewById(R.id.postprocessorCard)
        val title: TextView = view.findViewById(R.id.postprocessorTitle)
        val summary: TextView = view.findViewById(R.id.postprocessorSummary)
        val edit: ImageButton = view.findViewById(R.id.editPostprocessor)
        val remove: ImageButton = view.findViewById(R.id.removePostprocessor)
        val dragHandle: ImageButton = view.findViewById(R.id.dragPostprocessor)
        var moveEarlierAction = View.NO_ID
        var moveLaterAction = View.NO_ID

        fun clearMoveActions() {
            if (moveEarlierAction != View.NO_ID) {
                ViewCompat.removeAccessibilityAction(card, moveEarlierAction)
                moveEarlierAction = View.NO_ID
            }
            if (moveLaterAction != View.NO_ID) {
                ViewCompat.removeAccessibilityAction(card, moveLaterAction)
                moveLaterAction = View.NO_ID
            }
        }
    }
}
